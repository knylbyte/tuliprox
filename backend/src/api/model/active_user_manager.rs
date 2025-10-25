use std::borrow::Cow;
use crate::api::model::ActiveProviderManager;
use crate::api::model::SharedStreamManager;
use crate::model::Config;
use crate::model::ProxyUserCredentials;
use jsonwebtoken::get_current_timestamp;
use log::{debug, error, info};
use shared::model::{ActiveUserConnectionChange, StreamChannel, StreamInfo, UserConnectionPermission};
use shared::utils::{current_time_secs, default_grace_period_millis, default_grace_period_timeout_secs, sanitize_sensitive_info};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;


const USER_GC_TTL: u64 = 900;  // 15 Min
const USER_CON_TTL: u64 = 10_800;  // 3 hours
const USER_SESSION_LIMIT: usize = 50;

type ActiveUserConnectionChangeSender = tokio::sync::mpsc::Sender<ActiveUserConnectionChange>;
pub type ActiveUserConnectionChangeReceiver = tokio::sync::mpsc::Receiver<ActiveUserConnectionChange>;

macro_rules! active_user_manager_shared_impl {
    () => {
       #[inline]
        async fn get_active_connections(user: &Arc<RwLock<HashMap<String, UserConnectionData>>>) -> usize {
            user.read().await.iter().filter(|(_, c)| c.connections > 0).map(|(_, c)| c.connections as usize).sum()
        }

        #[inline]
        fn drop_connection(&self, addr: &str) {
             if let Err(e) = self.close_signal_tx.send(addr.to_string()) {
                 debug!("No active receivers for close signal ({addr}): {e:?}");
             }
        }

        async fn log_active_user(&self) {
          let user = Arc::clone(&self.user);
          let is_log_user_enabled = self.is_log_user_enabled();
          let user_connection_count = Self::get_active_connections(&user).await;
          let user_count = user.read().await.iter().filter(|(_, c)| c.connections > 0).count();
          let _= self.connection_change_tx.try_send(ActiveUserConnectionChange::Connections(user_count, user_connection_count));
          if is_log_user_enabled {
              info!("Active Users: {user_count}, Active User Connections: {user_connection_count}");
          }
        }

        pub async fn remove_connection(&self, addr: &str) {
             let username_opt = {
                self.user_by_addr.write().await.remove(addr)
            };

            if let Some(username) = username_opt {
                let mut user = self.user.write().await;
                if let Some(connection_data) = user.get_mut(&username) {
                    if connection_data.connections > 0 {
                        connection_data.connections -= 1;
                    }

                    if connection_data.connections < connection_data.max_connections {
                        connection_data.granted_grace = false;
                        connection_data.grace_ts = 0;
                    }
                    connection_data.streams.retain(|c| c.addr != addr);
                }
            }
            self.drop_connection(&addr);
            self.shared_stream_manager.release_connection(addr, true).await;
            self.provider_manager.release_connection(addr).await;
            let _= self.connection_change_tx.try_send(ActiveUserConnectionChange::Disconnected(addr.to_string()));
            self.log_active_user().await;
        }
    };
}

fn get_grace_options(config: &Config) -> (u64, u64) {
    let (grace_period_millis, grace_period_timeout_secs) = config.reverse_proxy.as_ref()
        .and_then(|r| r.stream.as_ref())
        .map_or_else(|| (default_grace_period_millis(), default_grace_period_timeout_secs()), |s| (s.grace_period_millis, s.grace_period_timeout_secs));
    (grace_period_millis, grace_period_timeout_secs)
}

struct ConnectionGuardUserManager {
    log_active_user: bool,
    user: Arc<RwLock<HashMap<String, UserConnectionData>>>,
    user_by_addr: Arc<RwLock<HashMap<String, String>>>,
    shared_stream_manager: Arc<SharedStreamManager>,
    provider_manager: Arc<ActiveProviderManager>,
    connection_change_tx: ActiveUserConnectionChangeSender,
    close_signal_tx: tokio::sync::broadcast::Sender<String>,
}

impl ConnectionGuardUserManager {
    active_user_manager_shared_impl!();
    fn is_log_user_enabled(&self) -> bool {
        self.log_active_user
    }
}

pub struct UserConnectionGuard {
    manager: Arc<ConnectionGuardUserManager>,
    // username: String,
    addr: String,
}
impl Drop for UserConnectionGuard {
    fn drop(&mut self) {
        let manager = self.manager.clone();
        let addr = self.addr.clone();
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(async move {
                manager.remove_connection(&addr).await;
            });
        } else {
            // Fallback: no runtime
            error!("Runtime not available, cannot cleanly remove connection for {addr}");
        }
    }
}

#[derive(Clone, Debug)]
pub struct UserSession {
    pub token: String,
    pub virtual_id: u32,
    pub provider: String,
    pub stream_url: String,
    pub addr: String,
    pub ts: u64,
    pub permission: UserConnectionPermission,
}

struct UserConnectionData {
    max_connections: u32,
    connections: u32,
    granted_grace: bool,
    grace_ts: u64,
    sessions: Vec<UserSession>,
    streams: Vec<StreamInfo>,
    ts: u64,
}

impl UserConnectionData {
    fn new(connections: u32, max_connections: u32) -> Self {
        Self {
            max_connections,
            connections,
            granted_grace: false,
            grace_ts: 0,
            sessions: Vec::new(),
            streams: Vec::new(),
            ts: current_time_secs(),
        }
    }

    fn add_session(&mut self, session: UserSession) {
        self.gc();
        self.sessions.push(session);
    }
    fn gc(&mut self) {
        if self.sessions.len() > USER_SESSION_LIMIT {
            self.sessions.sort_by_key(|e| std::cmp::Reverse(e.ts));
            self.sessions.truncate(USER_SESSION_LIMIT);
        }
    }
}

pub struct ActiveUserManager {
    grace_period_millis: AtomicU64,
    grace_period_timeout_secs: AtomicU64,
    log_active_user: AtomicBool,
    user: Arc<RwLock<HashMap<String, UserConnectionData>>>,
    user_by_addr: Arc<RwLock<HashMap<String, String>>>,
    gc_ts: Option<AtomicU64>,
    close_signal_tx: tokio::sync::broadcast::Sender<String>,
    shared_stream_manager: Arc<SharedStreamManager>,
    provider_manager: Arc<ActiveProviderManager>,
    connection_change_tx: ActiveUserConnectionChangeSender,
}

impl ActiveUserManager {
    pub fn new(config: &Config, shared_stream_manager: &Arc<SharedStreamManager>, provider_manager: &Arc<ActiveProviderManager>, connection_change_tx: ActiveUserConnectionChangeSender) -> Self {
        let log_active_user = config.log.as_ref().is_some_and(|l| l.log_active_user);
        let (grace_period_millis, grace_period_timeout_secs) = get_grace_options(config);
        let (close_signal_tx, _) = tokio::sync::broadcast::channel(10);
        Self {
            grace_period_millis: AtomicU64::new(grace_period_millis),
            grace_period_timeout_secs: AtomicU64::new(grace_period_timeout_secs),
            log_active_user: AtomicBool::new(log_active_user),
            user: Arc::new(RwLock::new(HashMap::new())),
            user_by_addr: Arc::new(RwLock::new(HashMap::new())),
            gc_ts: Some(AtomicU64::new(current_time_secs())),
            close_signal_tx,
            shared_stream_manager: Arc::clone(shared_stream_manager),
            provider_manager: Arc::clone(provider_manager),
            connection_change_tx,
        }
    }

    active_user_manager_shared_impl!();

    pub fn update_config(&self, config: &Config) {
        let log_active_user = config.log.as_ref().is_some_and(|l| l.log_active_user);
        let (grace_period_millis, grace_period_timeout_secs) = get_grace_options(config);
        self.grace_period_millis.store(grace_period_millis, Ordering::Relaxed);
        self.grace_period_timeout_secs.store(grace_period_timeout_secs, Ordering::Relaxed);
        self.log_active_user.store(log_active_user, Ordering::Relaxed);
    }

    fn clone_inner(&self) -> ConnectionGuardUserManager {
        ConnectionGuardUserManager {
            log_active_user: self.log_active_user.load(Ordering::Relaxed),
            user: Arc::clone(&self.user),
            user_by_addr: Arc::clone(&self.user_by_addr),
            shared_stream_manager: Arc::clone(&self.shared_stream_manager),
            provider_manager: Arc::clone(&self.provider_manager),
            connection_change_tx: self.connection_change_tx.clone(),
            close_signal_tx:  self.close_signal_tx.clone(),
        }
    }

    pub async fn user_connections(&self, username: &str) -> u32 {
        if let Some(connection_data) = self.user.read().await.get(username) {
            return connection_data.connections;
        }
        0
    }

    fn check_connection_permission(&self, username: &str, connection_data: &mut UserConnectionData) -> UserConnectionPermission {
        let current_connections = connection_data.connections;

        if current_connections < connection_data.max_connections {
            // Reset grace period because the user is back under max_connections
            connection_data.granted_grace = false;
            connection_data.grace_ts = 0;
            return UserConnectionPermission::Allowed;
        }

        let now = get_current_timestamp();
        // Check if user already used a grace period
        if connection_data.granted_grace {
            if current_connections > connection_data.max_connections && now - connection_data.grace_ts <= self.grace_period_timeout_secs.load(Ordering::Relaxed) {
                // Grace timeout, still active, deny connection
                debug!("User access denied, grace exhausted, too many connections: {username}");
                return UserConnectionPermission::Exhausted;
            }
            // Grace timeout expired, reset grace counters
            connection_data.granted_grace = false;
            connection_data.grace_ts = 0;
        }

        if self.grace_period_millis.load(Ordering::Relaxed) > 0 && current_connections == connection_data.max_connections {
            // Allow a grace period once
            connection_data.granted_grace = true;
            connection_data.grace_ts = now;
            debug!("Granted a grace period for user access: {username}");
            return UserConnectionPermission::GracePeriod;
        }

        // Too many connections, no grace allowed
        debug!("User access denied, too many connections: {username}");
        UserConnectionPermission::Exhausted
    }

    pub async fn connection_permission(
        &self,
        username: &str,
        max_connections: u32,
    ) -> UserConnectionPermission {
        if max_connections > 0 {
            if let Some(connection_data) = self.user.write().await.get_mut(username) {
                return self.check_connection_permission(username, connection_data);
            }
        }
        UserConnectionPermission::Allowed
    }

    pub async fn active_users(&self) -> usize {
        self.user.read().await.iter().filter(|(_, c)| c.connections > 0).count()
    }

    pub async fn active_connections(&self) -> usize {
        Self::get_active_connections(&self.user).await
    }

    pub async fn add_connection(&self, username: &str, max_connections: u32, addr: &str, provider: &str, stream_channel: StreamChannel, user_agent: Cow<'_, str>) -> UserConnectionGuard {
        let stream_info = StreamInfo::new(
            username,
            addr,
            provider,
            stream_channel,
            user_agent.to_string(),
        );
        {
            let mut user_map = self.user.write().await;
            if let Some(connection_data) = user_map.get_mut(username) {
                connection_data.connections += 1;
                connection_data.max_connections = max_connections;
                connection_data.streams.push(stream_info.clone());
            } else {
                let mut connection_data = UserConnectionData::new(1, max_connections);
                connection_data.streams.push(stream_info.clone());
                user_map.insert(username.to_string(), connection_data);
            }
        }

        {
            let mut user_by_addr = self.user_by_addr.write().await;
            user_by_addr.insert(addr.to_owned(), username.to_owned());
        }

        let _= self.connection_change_tx.try_send(ActiveUserConnectionChange::Connected(stream_info));
        self.log_active_user().await;

        UserConnectionGuard {
            manager: Arc::new(self.clone_inner()),
            addr: addr.to_owned(),
        }
    }

    fn is_log_user_enabled(&self) -> bool {
        self.log_active_user.load(Ordering::Relaxed)
    }

    fn new_user_session(session_token: &str, virtual_id: u32, provider: &str, stream_url: &str, addr: &str,
                        connection_permission: UserConnectionPermission) -> UserSession {
        UserSession {
            token: session_token.to_string(),
            virtual_id,
            provider: provider.to_string(),
            stream_url: stream_url.to_string(),
            addr: addr.to_string(),
            ts: current_time_secs(),
            permission: connection_permission,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_user_session(&self, user: &ProxyUserCredentials, session_token: &str, virtual_id: u32,
                                     provider: &str, stream_url: &str, addr: &str,
                                     connection_permission: UserConnectionPermission) -> String {
        self.gc();

        let username = user.username.clone();
        let mut user_map = self.user.write().await;
        let connection_data = user_map.entry(username.clone()).or_insert_with(|| {
            debug!("Creating session for user {username} with token {session_token} {}", sanitize_sensitive_info(stream_url));
            let mut data = UserConnectionData::new(0, user.max_connections);
            let session = Self::new_user_session(session_token, virtual_id, provider, stream_url, addr, connection_permission);
            data.add_session(session);
            data
        });

        // If a session exists, update it
        for session in &mut connection_data.sessions {
            if session.token == session_token {
                session.ts = current_time_secs();
                if session.stream_url != stream_url {
                    session.stream_url = stream_url.to_string();
                }
                if session.provider != provider {
                    session.provider = provider.to_string();
                }
                session.permission = connection_permission;
                debug!("Using session for user {} with token {session_token} {}", user.username, sanitize_sensitive_info(stream_url));
                return session.token.clone();
            }
        }

        // If no session exists, create one
        debug!("Creating session for user {} with token {session_token} {}", user.username, sanitize_sensitive_info(stream_url));
        let session = Self::new_user_session(session_token, virtual_id, provider, stream_url, addr, connection_permission);
        let token = session.token.clone();
        connection_data.add_session(session);
        token
    }

    pub async fn update_session_addr(&self, username: &str, token: &str, addr: &str) {
        let mut user_map = self.user.write().await;
        user_map.get_mut(username).and_then(|connection_data| {
            connection_data.sessions.iter_mut().find_map(|session| {
                if session.token == token {
                    let old_addr = session.addr.clone();
                    addr.clone_into(&mut session.addr);
                    Some(old_addr)
                } else {
                    None
                }
            })
        });
    }

    pub fn get_close_connection_channel(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.close_signal_tx.subscribe()
    }

    pub async fn get_and_update_user_session(&self, username: &str, token: &str) -> Option<UserSession> {
        self.update_user_session(username, token).await
    }

    async fn update_user_session(&self, username: &str, token: &str) -> Option<UserSession> {
        let mut users = self.user.write().await;
        if let Some(connection_data) = users.get_mut(username) {
            connection_data.ts = current_time_secs();

            // Search for index of session
            if let Some(index) = connection_data
                .sessions
                .iter()
                .position(|s| s.token == token)
            {
                // Refresh session last access
                connection_data.sessions[index].ts = current_time_secs();
                // Only re-evaluate permission for limited users during grace
                if connection_data.max_connections > 0
                   && connection_data.sessions[index].permission == UserConnectionPermission::GracePeriod
                {
                    let new_permission = self.check_connection_permission(username, connection_data);
                    connection_data.sessions[index].permission = new_permission;
                }
                return Some(connection_data.sessions[index].clone());
            }
        }
        None
    }

    pub async fn active_streams(&self) -> Vec<StreamInfo> {
        let user_map = self.user.read().await;
        let mut streams = Vec::new();
        for (_username, connection_data) in user_map.iter() {
            for stream in &connection_data.streams {
                streams.push(stream.clone());
            }
        }
        streams
    }

    fn gc(&self) {
        if let Some(gc_ts) = &self.gc_ts {
            let ts = gc_ts.load(Ordering::Acquire);
            let now = current_time_secs();

            if now - ts > USER_GC_TTL {
                if let Ok(mut users) = self.user.try_write() {
                    users.retain(|_k, v| now - v.ts < USER_CON_TTL && v.connections > 0);
                    for connection_data in users.values_mut() {
                        connection_data.sessions.retain(|s| now - s.ts < USER_CON_TTL);
                    }

                    gc_ts.store(now, Ordering::Release);
                }
            }
        }
    }
}

//
// mod tests {
//     use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
//     use std::time::Instant;
//     use std::thread;
//
//     fn benchmark(ordering: Ordering, iterations: usize) -> u128 {
//         let counter = Arc::new(AtomicUsize::new(0));
//         let start = Instant::now();
//
//         let handles: Vec<_> = (0..32)
//             .map(|_| {
//                 let counter_ref = Arc::clone(&counter);
//                 thread::spawn(move || {
//                     for _ in 0..iterations {
//                         counter_ref.fetch_add(1, ordering);
//                     }
//                 })
//             })
//             .collect();
//


//         for handle in handles {
//             handle.join().unwrap();
//         }
//
//         let duration = start.elapsed();
//         duration.as_millis()
//     }
//
//     #[test]
//     fn test_ordering() {
//         let iterations = 1_000_000;
//
//         let time_acqrel = benchmark(Ordering::SeqCst, iterations);
//         println!("AcqRel: {} ms", time_acqrel);
//
//         let time_seqcst = benchmark(Ordering::SeqCst, iterations);
//         println!("SeqCst: {} ms", time_seqcst);
//     }
//
// }
