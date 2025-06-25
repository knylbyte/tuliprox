use tokio::sync::{Mutex};
use std::sync::Arc;
use shared::error::TuliproxError;
use shared::model::UserConnectionPermission;
use crate::api::model::active_provider_manager::ActiveProviderManager;
use crate::api::model::active_user_manager::ActiveUserManager;
use crate::api::model::download::DownloadQueue;
use crate::api::model::streams::shared_stream_manager::SharedStreamManager;
use crate::model::{AppConfig, Config, HdHomeRunDeviceConfig};
use crate::tools::lru_cache::LRUResourceCache;

#[derive(Clone)]
pub struct AppState {
    pub app_config: Arc<AppConfig>,
    pub http_client: Arc<reqwest::Client>,
    pub downloads: Arc<DownloadQueue>,
    pub cache: Arc<Option<Mutex<LRUResourceCache>>>,
    pub shared_stream_manager: Arc<SharedStreamManager>,
    pub active_users: Arc<ActiveUserManager>,
    pub active_provider: Arc<ActiveProviderManager>,
}

impl AppState {

    pub async fn set_config(&self, config: Config) -> Result<(), TuliproxError> {
        self.active_users.update_config(&config);
        self.active_provider.update_config(&self.app_config).await;
        self.app_config.set_config(config)
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
