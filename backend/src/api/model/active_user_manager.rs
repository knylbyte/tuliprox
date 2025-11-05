use crate::api::model::{EventManager, EventMessage};
use crate::auth::Fingerprint;
use crate::model::Config;
use crate::model::ProxyUserCredentials;
use crate::utils::GeoIp;
use arc_swap::ArcSwapOption;
use jsonwebtoken::get_current_timestamp;
use log::{debug, info};
use shared::model::{ActiveUserConnectionChange, StreamChannel, StreamInfo, UserConnectionPermission};
use shared::utils::{current_time_secs, default_grace_period_millis, default_grace_period_timeout_secs, sanitize_sensitive_info, strip_port};
use std::borrow::Cow;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

const USER_GC_TTL: u64 = 900;  // 15 Min
const USER_CON_TTL: u64 = 10_800;  // 3 hours
const USER_SESSION_LIMIT: usize = 50;

fn get_grace_options(config: &Config) -> (u64, u64) {
    let (grace_period_millis, grace_period_timeout_secs) = config.reverse_proxy.as_ref()
        .and_then(|r| r.stream.as_ref())
        .map_or_else(|| (default_grace_period_millis(), default_grace_period_timeout_secs()), |s| (s.grace_period_millis, s.grace_period_timeout_secs));
    (grace_period_millis, grace_period_timeout_secs)
}

#[derive(Clone, Debug)]
pub struct UserSession {
    pub token: String,
    pub virtual_id: u32,
    pub provider: String,
    pub stream_url: String,
    pub addr: SocketAddr,
    pub ts: u64,
    pub permission: UserConnectionPermission,
}

#[derive(Debug)]
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

#[derive(Debug, Default)]
struct UserConnections {
    by_key: HashMap<String, UserConnectionData>,
    key_by_addr: HashMap<SocketAddr, String>,
}

pub struct ActiveUserManager {
    grace_period_millis: AtomicU64,
    grace_period_timeout_secs: AtomicU64,
    log_active_user: AtomicBool,
    gc_ts: Option<AtomicU64>,
    connections: RwLock<UserConnections>,
    event_manager: Arc<EventManager>,
    geo_ip: Arc<ArcSwapOption<GeoIp>>,
}

impl ActiveUserManager {
    pub fn new(config: &Config,
               geoip: &Arc<ArcSwapOption<GeoIp>>,
               event_manager: &Arc<EventManager>,) -> Self {
        let log_active_user: bool = config.log.as_ref().is_some_and(|l| l.log_active_user);
        let (grace_period_millis, grace_period_timeout_secs) = get_grace_options(config);

        Self {
            grace_period_millis: AtomicU64::new(grace_period_millis),
            grace_period_timeout_secs: AtomicU64::new(grace_period_timeout_secs),
            log_active_user: AtomicBool::new(log_active_user),
            connections: RwLock::new(UserConnections::default()),
            gc_ts: Some(AtomicU64::new(current_time_secs())),
            geo_ip: Arc::clone(geoip),
            event_manager: Arc::clone(event_manager),
        }
    }

    async fn log_active_user(&self) {
        let is_log_user_enabled = self.is_log_user_enabled();
        let (user_count, user_connection_count) = {
            let user_connections = self.connections.read().await;
            (user_connections.by_key.iter().filter(|(_, c)| c.connections > 0).count(),
             user_connections.key_by_addr.len())
        };
        self.event_manager.send_event(EventMessage::ActiveUser(ActiveUserConnectionChange::Connections(user_count, user_connection_count)));
        if is_log_user_enabled {
            info!("Active Users: {user_count}, Active User Connections: {user_connection_count}");
        }
    }

    pub async fn release_connection(&self, addr: &SocketAddr) {
        let log_active_user = {
            let mut user_connections = self.connections.write().await;
            let username_opt = {
                user_connections.key_by_addr.remove(addr)
            };

            if let Some(username) = username_opt.as_ref() {
                {
                    if let Some(connection_data) = user_connections.by_key.get_mut(username) {
                        if connection_data.connections > 0 {
                            connection_data.connections -= 1;
                        }

                        if connection_data.connections < connection_data.max_connections {
                            connection_data.granted_grace = false;
                            connection_data.grace_ts = 0;
                        }
                        connection_data.streams.retain(|c| c.addr != *addr);
                    }
                }
                true
            } else {
                false
            }
         };

        if log_active_user {
            self.log_active_user().await;
        }
    }

    pub fn update_config(&self, config: &Config) {
        let log_active_user = config.log.as_ref().is_some_and(|l| l.log_active_user);
        let (grace_period_millis, grace_period_timeout_secs) = get_grace_options(config);
        self.grace_period_millis.store(grace_period_millis, Ordering::Relaxed);
        self.grace_period_timeout_secs.store(grace_period_timeout_secs, Ordering::Relaxed);
        self.log_active_user.store(log_active_user, Ordering::Relaxed);
    }

    pub async fn user_connections(&self, username: &str) -> u32 {
        if let Some(connection_data) = self.connections.read().await.by_key.get(username) {
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
            if let Some(connection_data) = self.connections.write().await.by_key.get_mut(username) {
                return self.check_connection_permission(username, connection_data);
            }
        }
        UserConnectionPermission::Allowed
    }

    pub async fn active_users(&self) -> usize {
        self.connections.read().await.key_by_addr.len()
    }

    pub async fn active_connections(&self) -> usize {
        self.connections.read().await.by_key.values().map(|c| c.connections).sum::<u32>() as usize
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn add_connection(&self, username: &str, max_connections: u32, fingerprint: &Fingerprint,
                                provider: &str, stream_channel: StreamChannel, user_agent: Cow<'_, str>) -> StreamInfo {
        let country = {
            let geoip = self.geo_ip.load();
            if let Some(geoip_db) = (*geoip).as_ref() {
                geoip_db.lookup(&strip_port(&fingerprint.client_ip))
            } else {
                None
            }
        };

        let stream_info = StreamInfo::new(
            username,
            &fingerprint.addr,
            &fingerprint.client_ip,
            provider,
            stream_channel,
            user_agent.to_string(),
            country,
        );
        {
            let mut user_connections = self.connections.write().await;
            if let Some(connection_data) = user_connections.by_key.get_mut(username) {
                connection_data.connections += 1;
                connection_data.max_connections = max_connections;
                connection_data.streams.push(stream_info.clone());
            } else {
                let mut connection_data = UserConnectionData::new(1, max_connections);
                connection_data.streams.push(stream_info.clone());
                user_connections.by_key.insert(username.to_string(), connection_data);
            }

            user_connections.key_by_addr.insert(fingerprint.addr, username.to_string());
        }

        self.log_active_user().await;

        stream_info
    }

    fn is_log_user_enabled(&self) -> bool {
        self.log_active_user.load(Ordering::Relaxed)
    }

    fn new_user_session(session_token: &str, virtual_id: u32, provider: &str, stream_url: &str, addr: &SocketAddr,
                        connection_permission: UserConnectionPermission) -> UserSession {
        UserSession {
            token: session_token.to_string(),
            virtual_id,
            provider: provider.to_string(),
            stream_url: stream_url.to_string(),
            addr: *addr,
            ts: current_time_secs(),
            permission: connection_permission,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_user_session(&self, user: &ProxyUserCredentials, session_token: &str, virtual_id: u32,
                                     provider: &str, stream_url: &str, addr: &SocketAddr,
                                     connection_permission: UserConnectionPermission) -> String {
        self.gc();

        let username = user.username.clone();
        let mut user_connections = self.connections.write().await;
        let connection_data = user_connections.by_key.entry(username.clone()).or_insert_with(|| {
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

    pub async fn update_session_addr(&self, username: &str, token: &str, addr: &SocketAddr) {
        let mut user_connections = self.connections.write().await;
        if let Some(connection_data) = user_connections.by_key.get_mut(username) {
            if let Some(session) = connection_data.sessions.iter_mut().find(|s| s.token == token) {
                let previous_addr = session.addr;

                session.addr = *addr;
                for stream in &mut connection_data.streams {
                    if stream.addr == previous_addr {
                        stream.addr = *addr;
                    }
                }
            }
        }
    }

    pub async fn get_and_update_user_session(&self, username: &str, token: &str) -> Option<UserSession> {
        self.update_user_session(username, token).await
    }

    async fn update_user_session(&self, username: &str, token: &str) -> Option<UserSession> {
        let mut user_connections = self.connections.write().await;

        let connection_data = user_connections.by_key.get_mut(username)?;
        let now = current_time_secs();

        connection_data.ts = now;

        let session_index = connection_data.sessions.iter().position(|s| s.token == token)?;

        connection_data.sessions[session_index].ts = now;

        if connection_data.max_connections > 0
            && connection_data.sessions[session_index].permission == UserConnectionPermission::GracePeriod
        {
            let new_permission = self.check_connection_permission(username, connection_data);
            connection_data.sessions[session_index].permission = new_permission;
        }

        Some(connection_data.sessions[session_index].clone())
    }

    pub async fn active_streams(&self) -> Vec<StreamInfo> {
        let user_connections = self.connections.read().await;
        let mut streams = Vec::new();
        for (_username, connection_data) in user_connections.by_key.iter() {
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
                if let Ok(mut user_connections) = self.connections.try_write() {
                    let mut addrs_to_remove = Vec::new();

                    for (username, connection_data) in user_connections.by_key.iter_mut() {
                        let old_sessions: Vec<SocketAddr> = connection_data
                            .sessions
                            .iter()
                            .filter(|s| now - s.ts >= USER_CON_TTL)
                            .map(|s| s.addr)
                            .collect();

                        addrs_to_remove.extend(
                            old_sessions.into_iter().map(|addr| (addr, username.clone()))
                        );

                        connection_data.sessions.retain(|s| now - s.ts < USER_CON_TTL);
                    }

                    user_connections.by_key.retain(|_username, v| {
                        now - v.ts < USER_CON_TTL && v.connections > 0
                    });

                    for (addr, username) in addrs_to_remove {
                        if let Some(current_user) = user_connections.key_by_addr.get(&addr) {
                            if current_user == &username {
                                user_connections.key_by_addr.remove(&addr);
                            }
                        }
                    }

                    let valid_users: std::collections::HashSet<_> = user_connections.by_key.keys().cloned().collect();
                    user_connections.key_by_addr.retain(|_addr, username| valid_users.contains(username));

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
