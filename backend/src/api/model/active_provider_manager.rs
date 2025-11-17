use crate::api::model::provider_lineup_manager::{ProviderAllocation, ProviderLineupManager};
use crate::api::model::{EventManager, ProviderConfig};
use crate::model::{AppConfig, ConfigInput};
use log::{debug, info};
use shared::utils::{default_grace_period_millis, default_grace_period_timeout_secs, sanitize_sensitive_info};
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

        // IMPORTANT: Before acquiring a new connection, check if this addr is already in a shared stream
        // If so, remove it from the old shared stream to prevent connection leaks
        if let Some(old_key) = connections.shared.key_by_addr.get(addr).cloned() {
            info!("acquire_connection: addr {addr} found in shared stream {}, removing before acquiring new connection", sanitize_sensitive_info(&old_key));
            if let Some(shared_allocation) = connections.shared.by_key.get_mut(&old_key) {
                shared_allocation.connections.remove(addr);
                // If this was the last connection, release the provider allocation
                if shared_allocation.connections.is_empty() {
                    info!("acquire_connection: Last client left shared stream, releasing provider allocation");
                    let allocation = connections.shared.by_key.remove(&old_key).unwrap().allocation;
                    allocation.release().await;
                }
            }
            connections.shared.key_by_addr.remove(addr);
        }

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
        debug!("[DEBUG] release_connection called for addr={}", addr);
        let mut connections = self.connections.write().await;

        let handle = connections.single.remove(addr);
        if let Some(allocation) = handle {
            info!("Released provider connection (single) {:?} for {addr}", allocation.get_provider_name().unwrap_or_default());
            allocation.release().await;
            return;
        }

        let key = match connections.shared.key_by_addr.get(addr) {
            Some(k) => {
                debug!("[DEBUG] Found key for addr {} in shared: {}", addr, sanitize_sensitive_info(k));
                k.clone()
            }
            None => {
                debug!("[DEBUG] Addr {} NOT found in shared key_by_addr, nothing to release", addr);
                return;
            }
        };

        debug!("[DEBUG] Looking up shared allocation for key={}", sanitize_sensitive_info(&key));
        let mut released = false;
        if let Some(shared_alloc) = connections.shared.by_key.get_mut(&key) {
            let connections_before = shared_alloc.connections.len();
            let was_removed = shared_alloc.connections.remove(addr);
            let is_empty = shared_alloc.connections.is_empty();
            debug!("[DEBUG] Removed addr {} from shared connections: was_present={}, connections_before={}, connections_after={}, is_empty={}",
                addr, was_removed, connections_before, shared_alloc.connections.len(), is_empty);

            if was_removed && is_empty {
                info!("Releasing provider connection (shared, last client) for key={}", sanitize_sensitive_info(&key));
                shared_alloc.allocation.release().await;
                released = true;
            }
        } else {
            debug!("[DEBUG] No shared allocation found for key={}", sanitize_sensitive_info(&key));
        }

        if released {
            connections.shared.key_by_addr.remove(addr);
            connections.shared.by_key.remove(&key);
            debug!("[DEBUG] Cleaned up shared stream data for key={}", sanitize_sensitive_info(&key));
        }
    }

    pub async fn release_handle(&self, handle: &ProviderHandle) {
        debug!("[DEBUG] release_handle called with handle.id={}", handle.id);
        self.release_connection(&handle.id).await;
    }

    pub async fn make_shared_connection(&self, addr: &SocketAddr, key: &str) {
        let mut connections = self.connections.write().await;
        let handle = connections.single.remove(addr);
        if let Some(allocation) = handle {
            connections.shared.by_key.insert(key.to_string(), SharedAllocation { allocation, connections: HashSet::from([*addr]) });
            connections.shared.key_by_addr.insert(*addr, key.to_string());
        }
    }

    pub async fn add_shared_connection(&self, addr: &SocketAddr, key: &str) {
        debug!("[DEBUG] add_shared_connection called for addr={}, key={}", addr, sanitize_sensitive_info(key));
        let mut connections = self.connections.write().await;

        // Remove from single connections if present (subscriber joining existing shared stream)
        // This prevents double-release when the connection is closed
        if let Some(removed_allocation) = connections.single.remove(addr) {
            info!("add_shared_connection: Removed and releasing single connection for {addr} before adding to shared");
            // Release the allocation that was in single since we're joining an existing shared stream
            removed_allocation.release().await;
        }

        // Add to shared connections
        if let Some(shared_allocation) = connections.shared.by_key.get_mut(key) {
            shared_allocation.connections.insert(*addr);
            connections.shared.key_by_addr.insert(*addr, key.to_string());
            info!("add_shared_connection: Added {addr} to shared stream {}", sanitize_sensitive_info(key));
        } else {
            info!("add_shared_connection: WARNING - No shared allocation found for {}", sanitize_sensitive_info(key));
        }
    }

    pub async fn get_provider_connections_count(&self) -> usize {
        self.providers.active_connection_count().await
    }
}