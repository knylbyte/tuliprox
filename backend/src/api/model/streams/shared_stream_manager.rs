use crate::api::model::AppState;
use crate::api::model::StreamError;
use crate::api::model::STREAM_QUEUE_SIZE;
use crate::utils::debug_if_enabled;
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::model::BoxedProviderStream;
use crate::api::model::ProviderConnectionGuard;
use log::{debug, trace};
use crate::utils::{trace_if_enabled};
use shared::utils::sanitize_sensitive_info;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

///
/// Wraps a `ReceiverStream` as Stream<Item = Result<Bytes, `StreamError`>>
///
struct ReceiverStreamWrapper<S> {
    stream: S,
}

impl<S> Stream for ReceiverStreamWrapper<S>
where
    S: Stream<Item=Bytes> + Unpin,
{
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.stream).poll_next(cx) {
            Poll::Ready(Some(bytes)) => Poll::Ready(Some(Ok(bytes))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

fn convert_stream(stream: BoxStream<Bytes>) -> BoxStream<Result<Bytes, StreamError>> {
    ReceiverStreamWrapper { stream }.boxed()
}

type SubscriberId = String;

/// Represents the state of a shared provider URL.
///
/// - `headers`: The initial connection headers used during the setup of the shared stream.
pub struct SharedStreamState {
    headers: Vec<(String, String)>,
    buf_size: usize,
    provider_guard: Option<Arc<ProviderConnectionGuard>>,
    subscribers: RwLock<HashMap<SubscriberId, CancellationToken>>,
    broadcaster: tokio::sync::broadcast::Sender<Bytes>,
    stop_token: CancellationToken,
}

impl Drop for SharedStreamState {
    fn drop(&mut self) {
        if let Some(guard) = self.provider_guard.as_ref() {
            guard.force_release();
        }
    }
}

impl SharedStreamState {
    fn new(headers: Vec<(String, String)>, buf_size: usize,
           provider_guard: Option<Arc<ProviderConnectionGuard>>) -> Self {
        if let Some(guard) = &provider_guard {
            guard.disable_release();
        }
        let (broadcaster, _) = tokio::sync::broadcast::channel(buf_size);
        Self {
            headers,
            buf_size,
            provider_guard,
            subscribers: RwLock::new(HashMap::new()), //Arc::new(RwLock::new(Vec::new())),
            broadcaster,
            stop_token: CancellationToken::new(),
        }
    }

    async fn subscribe(&self, addr: &str, manager: Arc<SharedStreamManager>) -> BoxedProviderStream {
        let (client_tx, client_rx) = mpsc::channel(self.buf_size);
        let mut broadcast_rx = self.broadcaster.subscribe();
        let cancel_token = CancellationToken::new();
        self.subscribers.write().await.insert(addr.to_string(), cancel_token.clone());

        let address = addr.to_string();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;

                    () = cancel_token.cancelled() => {
                        debug!("Client disconnected from shared stream: {address}");
                        break;
                    }
                    result = broadcast_rx.recv() => {
                        match result {
                            Ok(data) => {
                                if let Err(err) = client_tx.send(data).await {
                                    debug!("Shared stream client send error: {address} {err}");
                                    break;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                trace!("Client lagged behind. Skipped {skipped} messages. {address}");
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
            manager.release_connection(&address, false).await;
        });
        convert_stream(ReceiverStream::new(client_rx).boxed())
    }

    fn broadcast<S, E>(
        &self,
        stream_url: &str,
        bytes_stream: S,
        shared_streams: Arc<SharedStreamManager>,
    )
    where
        S: Stream<Item=Result<Bytes, E>> + Unpin + 'static + Send,
        E: std::fmt::Debug + Send,
    {
        let mut source_stream = Box::pin(bytes_stream);
        let streaming_url = stream_url.to_string();
        let sender = self.broadcaster.clone();
        let stop_token = self.stop_token.clone();

        tokio::spawn(async move {
            let mut counter = 0u32;
            loop {
                tokio::select! {
                  biased;

                  () = stop_token.cancelled() => {
                       debug_if_enabled!("No shared stream subscribers left. Closing shared provider stream {}", sanitize_sensitive_info(&streaming_url));
                        break;
                  },

                  item = source_stream.next() => {
                     match item {
                        Some(Ok(data)) => {
                          match sender.send(data) {
                            Ok(clients) =>  {
                                if clients == 0 {
                                   debug_if_enabled!("No shared stream subscribers closing {}", sanitize_sensitive_info(&streaming_url));
                                   break;
                                }
                                counter += 1;
                                if counter >= 100 {
                                    tokio::task::yield_now().await;
                                    counter = 0;
                                }
                            }
                            Err(_e) => {
                                   debug_if_enabled!("Shared stream send error,no subscribers closing {}", sanitize_sensitive_info(&streaming_url));
                                   break;
                            }
                          }
                        }
                        Some(Err(e)) => {
                            trace!("Shared stream received error: {e:?}");
                        }
                        None => {
                            debug_if_enabled!("Source stream ended. Closing shared provider stream {}", sanitize_sensitive_info(&streaming_url));
                            break;
                        }
                    }
                  },
               }
            }
            debug_if_enabled!("Shared stream exhausted. Closing shared provider stream {}", sanitize_sensitive_info(&streaming_url));
            shared_streams.unregister(&streaming_url, false).await;
        });
    }
}

type SharedStreamRegister = HashMap<String, Arc<SharedStreamState>>;
type SharedStreamAddrRegister = HashMap<String, String>;


pub struct SharedStreamManager {
    shared_streams: RwLock<SharedStreamRegister>,
    shared_streams_by_addr: RwLock<SharedStreamAddrRegister>,
}

impl SharedStreamManager {
    pub(crate) fn new() -> Self {
        Self {
            shared_streams: RwLock::new(SharedStreamRegister::new()),
            shared_streams_by_addr: RwLock::new(SharedStreamAddrRegister::new()),
        }
    }

    pub async fn get_shared_state_headers(&self, stream_url: &str) -> Option<Vec<(String, String)>> {
        self.shared_streams.read().await.get(stream_url).map(|s| s.headers.clone())
    }

    pub async fn get_shared_state(&self, stream_url: &str) -> Option<Arc<SharedStreamState>> {
        self.shared_streams.read().await.get(stream_url).map(Arc::clone)
    }

    async fn unregister(&self, stream_url: &str, send_stop_signal: bool) {
        let mut broadcast_stop_sender = None;
        let shared_state = self.shared_streams.write().await.remove(stream_url);
        if let Some(shared_state) = shared_state {
            debug_if_enabled!("Unregistering shared stream {}", sanitize_sensitive_info(stream_url));
            if send_stop_signal {
                broadcast_stop_sender = Some(shared_state.stop_token.clone());
            }
            if let Some(guard) = &shared_state.provider_guard {
                guard.force_release();
            }
        }
        if let Some(stop_tx) = broadcast_stop_sender {
            trace_if_enabled!("Sending shared stream stop signal {}", sanitize_sensitive_info(stream_url));
            let () = stop_tx.cancel();
        }
    }

    pub async fn release_connection(&self, addr: &str, send_stop_signal: bool) {
        let stream_url = {
            self.shared_streams_by_addr.write().await.remove(addr)
        };
        let (client_stop_signal, should_unregister) = {
            if let Some(stream_url) = &stream_url {
                debug!("Release shared stream {addr}");
                let shared_state = {
                    self.shared_streams.read().await.get(stream_url).cloned()
                };

                if let Some(state) = shared_state {
                    let (tx, is_empty) = {
                        let mut subs = state.subscribers.write().await;
                        let tx = subs.remove(addr);
                        let is_empty = subs.is_empty();
                        (if send_stop_signal { tx } else { None }, is_empty)
                    };
                    (tx, is_empty)
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            }
        };

        if let Some(client_stop_signal) = client_stop_signal {
            let () = client_stop_signal.cancel();
        }

        if should_unregister {
            self.unregister(stream_url.as_ref().unwrap(), true).await;
        }

    }

    async fn subscribe_stream(&self, stream_url: &str, addr: Option<&str>, manager: Arc<SharedStreamManager>) -> Option<BoxedProviderStream> {
        let shared_stream_state = self.shared_streams.read().await.get(stream_url).map(Arc::clone);
        match shared_stream_state {
            None => None,
            Some(stream_state) => {
                if let Some(address) = addr {
                    debug_if_enabled!("Responding to existing shared client stream {}", sanitize_sensitive_info(stream_url));
                    self.shared_streams_by_addr.write().await.insert(address.to_string(), stream_url.to_owned());
                    let stream = stream_state.subscribe(address, manager).await;
                    Some(stream)
                } else {
                    None
                }
            }
        }
    }

    async fn register(&self, stream_url: &str, shared_state: Arc<SharedStreamState>) {
        let _ = self.shared_streams.write().await.insert(stream_url.to_string(), shared_state);
    }

    pub(crate) async fn register_shared_stream<S, E>(
        app_state: &AppState,
        stream_url: &str,
        bytes_stream: S,
        addr: Option<&str>,
        headers: Vec<(String, String)>,
        buffer_size: usize,
        provider_guard: Option<Arc<ProviderConnectionGuard>>) -> Option<BoxedProviderStream>
    where
        S: Stream<Item=Result<Bytes, E>> + Unpin + 'static + Send,
        E: std::fmt::Debug + Send,
    {
        let buf_size = STREAM_QUEUE_SIZE.max(buffer_size);
        let shared_state = Arc::new(SharedStreamState::new(headers, buf_size, provider_guard));
        app_state.shared_stream_manager.register(stream_url, Arc::clone(&shared_state)).await;
        debug_if_enabled!("Created shared provider stream {}", sanitize_sensitive_info(stream_url));
        let subscribed_stream = Self::subscribe_shared_stream(app_state, stream_url, addr).await;
        shared_state.broadcast(stream_url, bytes_stream, Arc::clone(&app_state.shared_stream_manager));
        subscribed_stream
    }

    /// Creates a broadcast notify stream for the given URL if a shared stream exists.
    pub async fn subscribe_shared_stream(
        app_state: &AppState,
        stream_url: &str,
        addr: Option<&str>,
    ) -> Option<BoxedProviderStream> {
        let manager = Arc::clone(&app_state.shared_stream_manager);
        app_state.shared_stream_manager.subscribe_stream(stream_url, addr, manager).await
    }
}