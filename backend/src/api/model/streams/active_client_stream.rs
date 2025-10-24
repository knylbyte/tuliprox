use crate::api::api_utils::StreamDetails;
use crate::api::model::AppState;
use crate::api::model::BoxedProviderStream;
use crate::api::model::StreamError;
use crate::api::model::TimedClientStream;
use crate::api::model::TransportStreamBuffer;
use crate::api::model::{ProviderConnectionGuard, UserConnectionGuard};
use crate::model::ProxyUserCredentials;
use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use log::{error, info};
use shared::model::{StreamChannel, UserConnectionPermission};
use std::pin::Pin;
use std::sync::atomic::AtomicU8;
use std::sync::{Arc};
use std::task::{Poll};
use futures::task::AtomicWaker;

const INNER_STREAM: u8 = 0_u8;
const GRACE_BLOCK_STREAM: u8 = 1_u8;
const USER_EXHAUSTED_STREAM: u8 = 2_u8;
const PROVIDER_EXHAUSTED_STREAM: u8 = 3_u8;

pub(in crate::api) struct ActiveClientStream {
    inner: BoxedProviderStream,
    send_custom_stream_flag: Option<Arc<AtomicU8>>,
    #[allow(unused)]
    user_connection_guard: Option<UserConnectionGuard>,
    #[allow(dead_code)]
    provider_connection_guard: Option<Arc<ProviderConnectionGuard>>,
    custom_video: (Option<TransportStreamBuffer>, Option<TransportStreamBuffer>),
    waker: Option<Arc<AtomicWaker>>,
}

impl ActiveClientStream {
    pub(crate) async fn new(mut stream_details: StreamDetails,
                      app_state: &AppState,
                      user: &ProxyUserCredentials,
                      connection_permission: UserConnectionPermission,
                      addr: &str,
                      stream_channel: StreamChannel) -> Self {
        if connection_permission == UserConnectionPermission::Exhausted {
            error!("Something is wrong this should not happen");
        }
        let grant_user_grace_period = connection_permission == UserConnectionPermission::GracePeriod;
        let username = user.username.as_str();
        let provider_name = stream_details
            .provider_connection_guard
            .as_ref()
            .and_then(|guard| guard.get_provider_name())
            .as_deref()
            .map_or_else(String::new, ToString::to_string);
        let user_connection_guard = Some(app_state.active_users.add_connection(username, user.max_connections, addr, &provider_name, stream_channel).await);
        let cfg = &app_state.app_config;
        let waker = Some(Arc::new(AtomicWaker::new()));
        let waker_clone = waker.clone();
        let grace_stop_flag = Self::stream_grace_period(app_state, &stream_details, grant_user_grace_period, user, addr, waker_clone.clone());
        let custom_response = cfg.custom_stream_response.load();
        let custom_video = custom_response.as_ref()
            .map_or((None, None), |c|
                (
                    c.user_connections_exhausted.clone(),
                    c.provider_connections_exhausted.clone()
                ));

        let stream = match stream_details.stream.take() {
            None => {
                if let Some(guard) = stream_details.provider_connection_guard.as_ref() {
                    guard.release();
                }
                futures::stream::empty::<Result<Bytes, StreamError>>().boxed()
            }
            Some(stream) => {
                match app_state.app_config.config.load().sleep_timer_mins {
                    None => stream,
                    Some(mins) => {
                        let secs = u32::try_from((u64::from(mins) * 60).min(u64::from(u32::MAX))).unwrap_or(0);
                        if secs > 0 {
                            TimedClientStream::new(stream, secs).boxed()
                        } else {
                            stream
                        }
                    }
                }
            }
        };

        Self {
            inner: stream,
            user_connection_guard,
            provider_connection_guard: stream_details.provider_connection_guard,
            send_custom_stream_flag: grace_stop_flag,
            custom_video,
            waker,
        }
    }

    fn stream_grace_period(app_state: &AppState,
                           stream_details: &StreamDetails,
                           user_grace_period: bool,
                           user: &ProxyUserCredentials,
                           addr: &str,
                           waker: Option<Arc<AtomicWaker>>) -> Option<Arc<AtomicU8>> {
        let active_users = Arc::clone(&app_state.active_users);
        let active_provider = Arc::clone(&app_state.active_provider);
        let shared_stream_manager = Arc::clone(&app_state.shared_stream_manager);

        let provider_grace_check = if stream_details.has_grace_period() && stream_details.input_name.is_some() {
            let provider_name = stream_details.input_name.as_deref().unwrap_or_default().to_string();
            Some(provider_name)
        } else {
            None
        };

        let user_max_connections = user.max_connections;
        let user_grace_check = if user_grace_period && user_max_connections > 0 {
            let user_name = user.username.clone();
            Some((user_name, user_max_connections))
        } else {
            None
        };

        if provider_grace_check.is_some() || user_grace_check.is_some() {
            let stream_strategy_flag = Arc::new(AtomicU8::new(GRACE_BLOCK_STREAM));
            let stream_strategy_flag_copy = Arc::clone(&stream_strategy_flag);
            let grace_period_millis = stream_details.grace_period_millis;

            let address = addr.to_string();
            let user_manager = Arc::clone(&active_users);
            let provider_manager = Arc::clone(&active_provider);
            let share_manager = Arc::clone(&shared_stream_manager);
            let reconnect_flag = stream_details.reconnect_flag.clone();
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(grace_period_millis)).await;

                let mut updated = false;
                if let Some((username, max_connections)) = user_grace_check {
                    let active_connections = user_manager.user_connections(&username).await;
                    if active_connections > max_connections {
                        stream_strategy_flag_copy.store(USER_EXHAUSTED_STREAM, std::sync::atomic::Ordering::Release);
                        info!("User connections exhausted for active clients: {username}");
                        updated = true;
                    }
                }

                if !updated {
                    if let Some(provider_name) = provider_grace_check {
                        if provider_manager.is_over_limit(&provider_name).await {
                            stream_strategy_flag_copy.store(PROVIDER_EXHAUSTED_STREAM, std::sync::atomic::Ordering::Release);
                            info!("Provider connections exhausted for active clients: {provider_name}");
                            updated = true;
                        }
                    }
                }
                if !updated {
                    stream_strategy_flag_copy.store(INNER_STREAM, std::sync::atomic::Ordering::Release);
                }

                if let Some(w) = waker.as_ref() {
                    w.wake();
                }

                if updated {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    share_manager.release_connection(&address, true).await;
                    provider_manager.release_connection(&address).await;
                     if let Some(flag) = reconnect_flag {
                         flag.notify();
                    }
                }
            });
            return Some(stream_strategy_flag);
        }
        None
    }
}
impl Stream for ActiveClientStream {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Option<Self::Item>> {
        let flag = {
            match &self.send_custom_stream_flag {
                Some(flag) => flag.load(std::sync::atomic::Ordering::Acquire),
                None => INNER_STREAM,
            }
        };

        if flag == GRACE_BLOCK_STREAM {
            if let Some(waker) = &self.waker {
                waker.register(cx.waker());
            }
            return Poll::Pending;
        }

        if flag == INNER_STREAM {
            return Pin::new(&mut self.inner).poll_next(cx);
        }

        let buffer_opt = match flag {
            USER_EXHAUSTED_STREAM => {
                self.custom_video.0.as_mut()
            }
            PROVIDER_EXHAUSTED_STREAM => {
                self.custom_video.1.as_mut()
            }
            _ => None,
        };

        if let Some(buffer) = buffer_opt {
            return Poll::Ready(Some(Ok(buffer.next_chunk())));
        }

        Poll::Ready(None)
    }
}
