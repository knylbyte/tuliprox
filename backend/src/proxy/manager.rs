use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use reqwest::Client;
use arc_swap::ArcSwap;
use sled::Db;
use crate::model::{AppConfig, ProxyServerConfig, IpCheckConfig};
use crate::utils::{request::create_client, ip_checker};
use bincode::serde::{encode_to_vec, decode_from_slice};
use bincode::config::standard;

#[derive(Clone)]
struct ProxyEntry {
    client: Arc<Client>,
    cfg: ProxyServerConfig,
    online: Arc<AtomicBool>,
}

pub struct ProxyManager {
    proxies: Vec<ProxyEntry>,
    weighted: Arc<RwLock<Vec<usize>>>,
    rr_index: AtomicUsize,
    session_db: Db,
    interval: u64,
    ipcheck: Option<IpCheckConfig>,
    base_client: ArcSwap<Client>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SessionValue {
    idx: usize,
    last_used: i64,
}

impl ProxyManager {
    pub fn new(app_config: &AppConfig) -> Self {
        let config = app_config.config.load();
        let pool_cfg = config.proxy_pool.clone();
        let ipcheck = config.ipcheck.clone();
        let base_client = ArcSwap::new(Arc::new(create_client(app_config, None).build().unwrap_or_else(|_| Client::new())));
        let mut proxies = Vec::new();
        if let Some(pool) = pool_cfg.as_ref() {
            for p in &pool.proxies {
                let client = create_client(app_config, Some(p)).build().unwrap_or_else(|_| Client::new());
                proxies.push(ProxyEntry { client: Arc::new(client), cfg: p.clone(), online: Arc::new(AtomicBool::new(false))});
            }
        }
        let db_path = format!("{}/session.db", config.working_dir);
        let session_db = sled::open(db_path).unwrap();
        let manager = Self {
            weighted: Arc::new(RwLock::new(Vec::new())),
            proxies,
            rr_index: AtomicUsize::new(0),
            session_db,
            interval: pool_cfg.as_ref().map_or(5, |p| p.interval_secs),
            ipcheck,
            base_client,
        };
        manager.spawn_health_task();
        manager
    }

    fn spawn_health_task(&self) {
        let proxies = self.proxies.clone();
        let weighted = Arc::clone(&self.weighted);
        let interval = self.interval;
        let ipcfg = self.ipcheck.clone();
        tokio::spawn(async move {
            loop {
                for p in &proxies {
                    let ok = if let Some(cfg) = ipcfg.as_ref() {
                        ip_checker::get_ips(&p.client, cfg).await.is_ok()
                    } else {
                        true
                    };
                    p.online.store(ok, Ordering::Relaxed);
                }
                let mut w = weighted.write().await;
                w.clear();
                for (idx, p) in proxies.iter().enumerate() {
                    if p.online.load(Ordering::Relaxed) {
                        for _ in 0..p.cfg.weight { w.push(idx); }
                    }
                }
                drop(w);
                tokio::time::sleep(Duration::from_secs(interval)).await;
            }
        });
    }

    pub async fn get_client_for_user(&self, user: &str) -> Option<Arc<Client>> {
        if let Ok(Some(val)) = self.session_db.get(user) {
            if let Ok((sess, _)) = decode_from_slice::<SessionValue, _>(&val, standard()) {
                if sess.last_used + 24*3600 > now_ts() {
                    if let Some(entry) = self.proxies.get(sess.idx) {
                        if entry.online.load(Ordering::Relaxed) {
                            if let Ok(bytes) = encode_to_vec(&SessionValue{idx: sess.idx, last_used: now_ts()}, standard()) {
                                let _ = self.session_db.insert(user, bytes);
                            }
                            return Some(Arc::clone(&entry.client));
                        }
                    }
                } else {
                    let _ = self.session_db.remove(user);
                }
            }
        }
        let weighted = self.weighted.read().await;
        if weighted.is_empty() { return None; }
        let idx = {
            let pos = self.rr_index.fetch_add(1, Ordering::Relaxed);
            weighted[pos % weighted.len()]
        };
        drop(weighted);
        if let Some(entry) = self.proxies.get(idx) {
            if let Ok(bytes) = encode_to_vec(&SessionValue{idx, last_used: now_ts()}, standard()) {
                let _ = self.session_db.insert(user, bytes);
            }
            Some(Arc::clone(&entry.client))
        } else {
            None
        }
    }

    pub fn base_client(&self) -> Arc<Client> {
        Arc::clone(&self.base_client.load())
    }
}

fn now_ts() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

