use tokio::sync::{Mutex};
use std::sync::Arc;
use std::time::Duration;
use arc_swap::{ArcSwap, ArcSwapAny};
use log::error;
use reqwest::Client;
use shared::error::TuliproxError;
use shared::model::UserConnectionPermission;
use crate::api::model::active_provider_manager::ActiveProviderManager;
use crate::api::model::active_user_manager::ActiveUserManager;
use crate::api::model::download::DownloadQueue;
use crate::api::model::streams::shared_stream_manager::SharedStreamManager;
use crate::model::{AppConfig, Config, HdHomeRunDeviceConfig};
use crate::tools::lru_cache::LRUResourceCache;
use crate::utils::request::create_client;

pub fn create_http_client(app_config: &AppConfig) -> Client {
    let mut builder = create_client(app_config).http1_only();
    let config = app_config.config.load();// because of RAII connection dropping
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


#[derive(Clone)]
pub struct AppState {
    pub app_config: Arc<AppConfig>,
    pub http_client: Arc<ArcSwap<reqwest::Client>>,
    pub downloads: Arc<DownloadQueue>,
    pub cache: Arc<ArcSwapAny<Option<Arc<Mutex<LRUResourceCache>>>>>,
    pub shared_stream_manager: Arc<SharedStreamManager>,
    pub active_users: Arc<ActiveUserManager>,
    pub active_provider: Arc<ActiveProviderManager>,
}

impl AppState {

    pub async fn set_config(&self, config: Config) -> Result<(), TuliproxError> {
        config.update_runtime();
        self.active_users.update_config(&config);
        self.active_provider.update_config(&self.app_config).await;
        self.app_config.set_config(config)?;
        self.update_config().await;
        Ok(())
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
}

#[derive(Clone)]
pub struct HdHomerunAppState {
    pub app_state: Arc<AppState>,
    pub device: Arc<HdHomeRunDeviceConfig>,
}
