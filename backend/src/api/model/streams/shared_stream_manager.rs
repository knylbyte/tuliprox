use crate::api::model::app_state::AppState;
use crate::api::model::stream_error::StreamError;
use crate::api::model::streams::provider_stream_factory::STREAM_QUEUE_SIZE;
use crate::utils::debug_if_enabled;
use bytes::Bytes;
use futures::stream::{BoxStream, FuturesUnordered};
use futures::{Stream, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

use crate::api::model::stream::BoxedProviderStream;
use dashmap::DashMap;
use log::trace;
use shared::utils::sanitize_sensitive_info;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio_stream::wrappers::ReceiverStream;
use crate::api::model::active_provider_manager::ProviderConnectionGuard;

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
struct SharedStreamState {
    headers: Vec<(String, String)>,
    buf_size: usize,
    provider_guard: Option<ProviderConnectionGuard>,
    subscribers: Arc<DashMap<SubscriberId, Sender<Bytes>>>, //Arc<RwLock<Vec<Sender<Bytes>>>>,
}

impl SharedStreamState {
    fn new(headers: Vec<(String, String)>, buf_size: usize,
           provider_guard: Option<ProviderConnectionGuard>) -> Self {
        if let Some(guard) = &provider_guard {
            guard.disable_release();
        }
        Self {
            headers,
            buf_size,
            provider_guard,
            subscribers: Arc::new(DashMap::new()), //Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn subscribe(&self, addr: &str) -> BoxedProviderStream {
        let (tx, rx) = mpsc::channel(self.buf_size);
        self.subscribers.insert(addr.to_string(), tx);
        convert_stream(ReceiverStream::new(rx).boxed())
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
        let subscribers = self.subscribers.clone();
        let streaming_url = stream_url.to_string();

        tokio::spawn(async move {
            while let Some(item) = source_stream.next().await {
                let Ok(data) = item else {
                    trace!("Shared stream received Err, skipping...");
                    continue;
                };

                if subscribers.is_empty() {
                    debug_if_enabled!("No active subscribers. Closing shared provider stream {}", sanitize_sensitive_info(&streaming_url));
                    shared_streams.unregister(&streaming_url);
                    break;
                }

                // Fast-path: at least one has capacity
                if subscribers.iter().any(|sender| sender.value().capacity() > 0) {
                    // Try sending immediately
                    subscribers.retain(|_id, sender| match sender.try_send(data.clone()) {
                        Ok(()) => true,
                        Err(TrySendError::Closed(_)) => false,
                        Err(err) => {
                            trace!("broadcast try_send error: {err:?}");
                            true
                        }
                    });
                    continue;
                }

                // All are full → wait until one becomes available
                let mut futures = FuturesUnordered::new();
                for sender in subscribers.iter() {
                    let tx = sender.value().clone();
                    futures.push(async move {
                        match tx.reserve().await {
                            Ok(permit) => {
                                drop(permit);
                                Ok(())
                            }
                            Err(_) => Err(()),
                        }
                    });
                }

                let mut has_fillable_subscriber = false;
                while let Some(result) = futures.next().await {
                    if let Ok(()) = result {
                        has_fillable_subscriber = true;
                        break;
                    }
                }

                if !has_fillable_subscriber {
                    debug_if_enabled!("All subscribers closed. Shutting down shared provider stream {}",sanitize_sensitive_info(&streaming_url));
                    shared_streams.unregister(&streaming_url);
                    break;
                }

                // Re-validate subscribers and send again
                subscribers.retain(|_id, sender| match sender.try_send(data.clone()) {
                    Ok(()) => true,
                    Err(TrySendError::Closed(_)) => false,
                    Err(err) => {
                        trace!("broadcast try_send error after reserve: {err:?}");
                        true
                    }
                });
            }

            debug_if_enabled!("Shared stream exhausted. Closing shared provider stream {}", sanitize_sensitive_info(&streaming_url));
            shared_streams.unregister(&streaming_url);
        });
    }
}

type SharedStreamRegister = DashMap<String, SharedStreamState>;

pub struct SharedStreamManager {
    shared_streams: SharedStreamRegister,
    shared_streams_by_addr: DashMap<String, String>,
}

impl SharedStreamManager {
    pub(crate) fn new() -> Self {
        Self {
            shared_streams: DashMap::new(),
            shared_streams_by_addr: DashMap::new(),
        }
    }

    pub fn get_shared_state_headers(&self, stream_url: &str) -> Option<Vec<(String, String)>> {
        self.shared_streams.get(stream_url).map(|s| s.headers.clone())
    }

    fn unregister(&self, stream_url: &str) {
        if let Some((_, shared_state)) = self.shared_streams.remove(stream_url) {
            if let Some(guard) = &shared_state.provider_guard {
                guard.force_release();
            }
        }
    }

    pub fn release_connection(&self, addr: &str) {
        let stream_url = self.shared_streams_by_addr.get(addr).map(|a| a.value().clone());
        if let Some(stream_url) = stream_url {
            let mut drop_state = false;
            if let Some(entry) = self.shared_streams.get_mut(&stream_url) {
                let shared_state = &*entry;
                if let Some((_, tx)) = shared_state.subscribers.remove(addr) {
                    drop(tx);
                }
                if shared_state.subscribers.is_empty() {
                    drop_state = true;
                }
            }
            if drop_state {
                self.unregister(&stream_url);
            }
        }
    }

    fn subscribe_stream(&self, stream_url: &str, addr: &str) -> Option<BoxedProviderStream> {
        let stream_data = self.shared_streams.get(stream_url)?.subscribe(addr);
        Some(stream_data)
    }

    fn register(&self, stream_url: &str, shared_state: SharedStreamState) {
        let _ = self.shared_streams.insert(stream_url.to_string(), shared_state);
    }

    pub(crate) fn subscribe<S, E>(
        app_state: &AppState,
        stream_url: &str,
        addr: &str,
        bytes_stream: S,
        headers: Vec<(String, String)>,
        buffer_size: usize,
        provider_guard: Option<ProviderConnectionGuard>) -> Option<BoxedProviderStream>
    where
        S: Stream<Item=Result<Bytes, E>> + Unpin + 'static + Send,
        E: std::fmt::Debug + Send,
    {
        let buf_size = std::cmp::max(buffer_size, STREAM_QUEUE_SIZE);
        let shared_state = SharedStreamState::new(headers, buf_size, provider_guard);
        shared_state.broadcast(stream_url, bytes_stream, Arc::clone(&app_state.shared_stream_manager));
        app_state.shared_stream_manager.register(stream_url, shared_state);
        debug_if_enabled!("Created shared provider stream {}", sanitize_sensitive_info(stream_url));
        Self::subscribe_shared_stream(app_state, stream_url, addr)
    }

    /// Creates a broadcast notify stream for the given URL if a shared stream exists.
    pub fn subscribe_shared_stream(
        app_state: &AppState,
        stream_url: &str,
        addr: &str,
    ) -> Option<BoxedProviderStream> {
        debug_if_enabled!("Responding existing shared client stream {}", sanitize_sensitive_info(stream_url));
        app_state.shared_stream_manager.subscribe_stream(stream_url, addr)
    }
}