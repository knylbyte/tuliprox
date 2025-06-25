use crate::api::model::active_provider_manager::ActiveProviderManager;
use crate::api::model::active_user_manager::ActiveUserManager;
use crate::api::model::download::DownloadQueue;
use crate::api::model::streams::shared_stream_manager::SharedStreamManager;
use crate::api::scheduler::exec_scheduler;
use crate::model::{AppConfig, Config, HdHomeRunConfig, HdHomeRunDeviceConfig, ProcessTargets, ScheduleConfig};
use crate::tools::lru_cache::LRUResourceCache;
use crate::utils::request::create_client;
use arc_swap::{ArcSwap, ArcSwapAny};
use log::error;
use reqwest::Client;
use shared::error::TuliproxError;
use shared::model::UserConnectionPermission;
use shared::utils::small_vecs_equal_unordered;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub(in crate::api) struct UpdateChanges {
    scheduler: bool,
    hdhomerun: bool,
}

pub async fn update_app_state(app_state: &Arc<AppState>, config: Config) -> Result<(), TuliproxError> {
    let updates = app_state.set_config(config).await?;
    start_services(app_state, &updates);
    Ok(())
}

fn start_services(app_state: &Arc<AppState>, changes: &UpdateChanges) {
    if changes.scheduler {
        exec_scheduler(&Arc::clone(&app_state.http_client.load()), &app_state.app_config,
                       &app_state.forced_targets.load(), &app_state.cancel_tokens.load().scheduler);
    }

    if changes.hdhomerun && app_state.app_config.api_proxy.load().is_some() {
        let mut infos = Vec::new();
        crate::api::main_api::start_hdhomerun(&app_state.app_config, app_state, &mut infos, &app_state.cancel_tokens.load().hdhomerun);
    }
}

pub fn create_http_client(app_config: &AppConfig) -> Client {
    let mut builder = create_client(app_config).http1_only();
    let config = app_config.config.load(); // because of RAII connection dropping
    if config.connect_timeout_secs > 0 {
        builder = builder.connect_timeout(Duration::from_secs(u64::from(config.connect_timeout_secs)));
    }
    builder.build().unwrap_or_else(|_| Client::new())
}

pub fn create_cache(config: &Config) -> Option<Arc<Mutex<LRUResourceCache>>> {
    let lru_cache = config.reverse_proxy.as_ref().and_then(|r| r.cache.as_ref()).and_then(|c| if c.enabled {
        Some(LRUResourceCache::new(c.size, c.dir.as_str()))
    } else { None });
    let cache_enabled = lru_cache.is_some();
    if cache_enabled {
        if let Some(res_cache) = lru_cache {
            let cache = Arc::new(Mutex::new(res_cache));
            let cache_scanner = Arc::clone(&cache);
            tokio::spawn(async move {
                let mut c = cache_scanner.lock().await;
                if let Err(err) = (*c).scan() {
                    error!("Failed to scan cache {err}");
                }
            });
            return Some(cache);
        }
    }
    None
}

pub struct CancelTokens {
    pub(crate) scheduler: CancellationToken,
    pub(crate) hdhomerun: CancellationToken,
}
impl Default for CancelTokens {
    fn default() -> Self {
        Self {
            scheduler: CancellationToken::new(),
            hdhomerun: CancellationToken::new(),
        }
    }
}

macro_rules! change_detect {
    ($fn_name:ident, $a:expr, $b: expr) => {
        match ($a, $b) {
            (None, None) => false,
            (Some(_), None) |
            (None, Some(_)) => true,
            (Some(o), Some(n)) => $fn_name(o, n),
        }
    };
}

#[derive(Clone)]
pub struct AppState {
    pub forced_targets: Arc<ArcSwap<ProcessTargets>>, // as program arguments
    pub app_config: Arc<AppConfig>,
    pub http_client: Arc<ArcSwap<Client>>,
    pub downloads: Arc<DownloadQueue>,
    pub cache: Arc<ArcSwapAny<Option<Arc<Mutex<LRUResourceCache>>>>>,
    pub shared_stream_manager: Arc<SharedStreamManager>,
    pub active_users: Arc<ActiveUserManager>,
    pub active_provider: Arc<ActiveProviderManager>,
    pub cancel_tokens: Arc<ArcSwap<CancelTokens>>,
}

impl AppState {
    pub(in crate::api::model) async fn set_config(&self, config: Config) -> Result<UpdateChanges, TuliproxError> {
        let changes = self.detect_changes(&config);
        config.update_runtime();
        self.active_users.update_config(&config);
        self.active_provider.update_config(&self.app_config).await;
        self.app_config.set_config(config)?;
        self.update_config().await;
        Ok(changes)
    }

    async fn update_config(&self) {
        // client
        let client = create_http_client(&self.app_config);
        self.http_client.store(Arc::new(client));

        // cache
        let config = self.app_config.config.load();
        let (enabled, size, cache_dir) = config
            .reverse_proxy
            .as_ref()
            .and_then(|r| r.cache.as_ref())
            .map_or((false, 0, ""), |c| (c.enabled, c.size, c.dir.as_str()));


        if let Some(cache) = self.cache.load().as_ref() {
            if enabled {
                cache.lock().await.update_config(size, cache_dir);
            } else {
                self.cache.store(None);
            }
        } else {
            let cache = create_cache(&config);
            self.cache.store(cache);
        }
    }

    pub async fn get_active_connections_for_user(&self, username: &str) -> u32 {
        self.active_users.user_connections(username).await
    }

    pub async fn get_connection_permission(&self, username: &str, max_connections: u32) -> UserConnectionPermission {
        self.active_users.connection_permission(username, max_connections).await
    }

    fn detect_changes(&self, config: &Config) -> UpdateChanges {
        let old_config = self.app_config.config.load();
        let changed_schedules = change_detect!(schedules_changed, old_config.schedules.as_ref(), config.schedules.as_ref());
        let changed_hdhomerun = change_detect!(hdhomerun_changed, old_config.hdhomerun.as_ref(), config.hdhomerun.as_ref());

        if changed_schedules || changed_hdhomerun {
            let cancel_tokens = self.cancel_tokens.load();
            if changed_schedules {
                cancel_tokens.scheduler.cancel();
            }
            if changed_hdhomerun {
                cancel_tokens.hdhomerun.cancel();
            }

            let tokens = CancelTokens {
                scheduler: if changed_schedules { CancellationToken::default() } else { cancel_tokens.scheduler.clone() },
                hdhomerun: if changed_hdhomerun { CancellationToken::default() } else { cancel_tokens.hdhomerun.clone() },
            };
            self.cancel_tokens.store(Arc::new(tokens));
        }
        UpdateChanges {
            scheduler: changed_schedules,
            hdhomerun: changed_hdhomerun,
        }
    }
}

fn schedules_changed(a: &[ScheduleConfig], b: &[ScheduleConfig]) -> bool {
    if a.len() != b.len() {
        return true;
    }
    for schedule in a {
        if let Some(found) = b.iter().find(|&s| s.schedule == schedule.schedule) {
            match (schedule.targets.as_ref(), found.targets.as_ref()) {
                (None, None) => return false,
                (Some(_targets), None) |
                (None, Some(_targets)) => return true,
                (Some(a_targets), Some(b_targets)) => {
                    if !small_vecs_equal_unordered(a_targets, b_targets) {
                        return true;
                    }
                }
            }
        } else {
            return true;
        }
    }
    false
}

fn hdhomerun_changed(a: &HdHomeRunConfig, b: &HdHomeRunConfig) -> bool {
    if a.enabled != b.enabled || a.auth != b.auth {
        return true;
    }
    if !small_vecs_equal_unordered(a.devices.as_ref(), b.devices.as_ref()) {
        return true;
    }
    false
}

#[derive(Clone)]
pub struct HdHomerunAppState {
    pub app_state: Arc<AppState>,
    pub device: Arc<HdHomeRunDeviceConfig>,
}
