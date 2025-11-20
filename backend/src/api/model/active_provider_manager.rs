use crate::api::model::provider_lineup_manager::{ProviderAllocation, ProviderLineupManager};
use crate::api::model::{EventManager, ProviderConfig};
use crate::model::{AppConfig, ConfigInput};
use log::{debug, error};
use shared::utils::{default_grace_period_millis, default_grace_period_timeout_secs};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type ProviderConnectionId = SocketAddr;

#[derive(Debug, Clone)]
pub struct ProviderHandle {
    pub id: ProviderConnectionId,
    pub allocation: ProviderAllocation,
}

impl ProviderHandle {
    pub fn new(id: ProviderConnectionId, allocation: ProviderAllocation) -> Self {
        Self { id, allocation }
    }
}

#[derive(Debug, Clone)]
struct SharedAllocation {
    allocation: ProviderAllocation,
    connections: HashSet<ProviderConnectionId>,
}

#[derive(Debug, Clone, Default)]
struct SharedConnections {
    by_key: HashMap<String, SharedAllocation>,
    key_by_addr: HashMap<SocketAddr, String>,
}

#[derive(Debug, Clone, Default)]
struct Connections {
    single: HashMap<ProviderConnectionId, ProviderAllocation>,
    shared: SharedConnections,
}

pub struct ActiveProviderManager {
    providers: ProviderLineupManager,
    connections: RwLock<Connections>,
}

impl ActiveProviderManager {
    pub fn new(cfg: &AppConfig, event_manager: &Arc<EventManager>) -> Self {
        let (grace_period_millis, grace_period_timeout_secs) = Self::get_grace_options(cfg);
        let inputs = Self::get_config_inputs(cfg);
        Self {
            providers: ProviderLineupManager::new(inputs, grace_period_millis, grace_period_timeout_secs, event_manager),
            connections: RwLock::new(Connections::default()),
        }
    }

    fn get_config_inputs(cfg: &AppConfig) -> Vec<Arc<ConfigInput>> {
        cfg.sources.load().sources
            .iter().flat_map(|s| s.inputs.iter().map(Arc::clone)).collect()
    }

    fn get_grace_options(cfg: &AppConfig) -> (u64, u64) {
        let (grace_period_millis, grace_period_timeout_secs) = cfg.config.load().reverse_proxy.as_ref()
            .and_then(|r| r.stream.as_ref())
            .map_or_else(|| (default_grace_period_millis(), default_grace_period_timeout_secs()), |s| (s.grace_period_millis, s.grace_period_timeout_secs));
        (grace_period_millis, grace_period_timeout_secs)
    }

    pub async fn update_config(&self, cfg: &AppConfig) {
        let (grace_period_millis, grace_period_timeout_secs) = Self::get_grace_options(cfg);
        let inputs = Self::get_config_inputs(cfg);
        self.providers.update_config(inputs, grace_period_millis, grace_period_timeout_secs).await;
    }

    async fn acquire_connection_inner(&self, provider_or_input_name: &str, addr: &SocketAddr, force: bool) -> Option<ProviderHandle> {
        // Lock connections
        let mut connections = self.connections.write().await;

        // Call the specific acquisition function
        let allocation = if force {
            self.providers.force_exact_acquire_connection(provider_or_input_name).await
        } else {
            self.providers.acquire_connection(provider_or_input_name).await
        };

        match &allocation {
            ProviderAllocation::Exhausted => {}
            ProviderAllocation::Available(_) | ProviderAllocation::GracePeriod(_) => {
                let provider_name = allocation.get_provider_name().unwrap_or_default();

                if let Some(old) = connections.single.insert(*addr, allocation.clone()) {
                    crate::utils::trace_if_enabled!(
                    "register_connection: address {addr} already had a allocation for provider {:?} — forcing release on the old allocation",
                    old.get_provider_name().unwrap_or_default()
                );
                    old.release().await;
                }

                debug!("Added provider connection {provider_name:?} for {addr}");
                return Some(ProviderHandle::new(*addr, allocation));
            }
        }

        None
    }

    pub async fn force_exact_acquire_connection(&self, provider_name: &str, addr: &SocketAddr) -> Option<ProviderHandle> {
        self.acquire_connection_inner(provider_name, addr, true).await
    }

    // Returns the next available provider connection
    pub async fn acquire_connection(&self, input_name: &str, addr: &SocketAddr) -> Option<ProviderHandle> {
        self.acquire_connection_inner(input_name, addr, false).await
    }

    // This method is used for redirects to cycle through provider
    pub async fn get_next_provider(&self, provider_name: &str) -> Option<Arc<ProviderConfig>> {
        self.providers.get_next_provider(provider_name).await
    }

    pub async fn active_connections(&self) -> Option<HashMap<String, usize>> {
        self.providers.active_connections().await
    }

    pub async fn is_over_limit(&self, provider_name: &str) -> bool {
        self.providers.is_over_limit(provider_name).await
    }

    pub async fn release_connection(&self, addr: &SocketAddr) {
        let mut connections = self.connections.write().await;

        // try to release the single connection (not shared)
        let handle = connections.single.remove(addr);
        if let Some(allocation) = handle {
            debug!("Released provider connection {:?} for {addr}", allocation.get_provider_name().unwrap_or_default());
            allocation.release().await;
            return;
        }

        let key = match connections.shared.key_by_addr.get(addr) {
            Some(k) => k.clone(),
            None => return,
        };

        let mut released = false;
        if let Some(connections) = connections.shared.by_key.get_mut(&key) {
            if connections.connections.remove(addr) && connections.connections.is_empty() {
                connections.allocation.release().await;
                released = true;
            }
        }

        if released {
            connections.shared.key_by_addr.remove(addr);
            connections.shared.by_key.remove(&key);
        }
    }

    pub async fn release_handle(&self, handle: &ProviderHandle) {
        self.release_connection(&handle.id).await;
    }

    pub async fn make_shared_connection(&self, addr: &SocketAddr, key: &str) {
        let mut connections = self.connections.write().await;
        let handle = connections.single.remove(addr);
        if let Some(allocation) = handle {
            debug!("Shared connection: Promoted connection {addr} to shared with key {key:?}");
            connections.shared.by_key.insert(key.to_string(), SharedAllocation { allocation, connections: HashSet::from([*addr]) });
            connections.shared.key_by_addr.insert(*addr, key.to_string());
        }
    }

    pub async fn add_shared_connection(&self, addr: &SocketAddr, key: &str) {
        let mut connections = self.connections.write().await;
        if let Some(shared_allocation) = connections.shared.by_key.get_mut(key) {
            debug!("Shared connection: Promoted connection {addr} to shared with key {key:?}");
            shared_allocation.connections.insert(*addr);
            connections.shared.key_by_addr.insert(*addr, key.to_string());
        } else {
          error!("Failed to add shared connection for {addr}: url: {key:?} not found");
        }
    }

    pub async fn get_provider_connections_count(&self) -> usize {
        self.providers.active_connection_count().await
    }
}