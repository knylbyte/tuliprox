use crate::api::model::{AppState};
use crate::api::model::StreamError;
use crate::utils::debug_if_enabled;
use bytes::{Bytes};
use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use crate::api::model::BoxedProviderStream;
use crate::api::model::ProviderConnectionGuard;
use log::{debug, trace};
use crate::utils::{trace_if_enabled};
use shared::utils::sanitize_sensitive_info;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::sync::mpsc::Sender;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use crate::api::model::streams::buffered_stream::CHANNEL_SIZE;

// TODO make this configurable
const  MIN_SHARED_BUFFER_SIZE: usize = 1024 * 1024 * 12; // 12 MB

const YIELD_COUNTER:usize = 200;

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


struct BurstBuffer {
    buffer: VecDeque<Bytes>,
    buffer_size: usize,
    current_bytes: usize,
}

#[allow(clippy::missing_fields_in_debug)]
impl Debug for BurstBuffer {

    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("BurstBuffer")
            .field("buffer_size", &self.buffer_size)
            .field("current_bytes", &self.current_bytes)
            .finish()
    }
}

impl BurstBuffer {
    pub fn new(buf_size: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(buf_size),
            buffer_size: buf_size,
            current_bytes: 0,
        }
    }

    pub fn snapshot(&self) -> VecDeque<Bytes> {
        self.buffer.iter().cloned().collect::<VecDeque<Bytes>>()
    }

    pub fn push(&mut self, packet: &Bytes) {
        while self.current_bytes > self.buffer_size {
            if let Some(popped) = self.buffer.pop_front() {
                self.current_bytes -= popped.len();
            } else {
                self.current_bytes  = 0;
                break;
            }
        }
        self.current_bytes += packet.len();
        self.buffer.push_back(packet.clone());
    }
}

/// Represents the state of a shared provider URL.
///
/// - `headers`: The initial connection headers used during the setup of the shared stream.
#[derive(Debug)]
pub struct SharedStreamState {
    headers: Vec<(String, String)>,
    buf_size: usize,
    provider_guard: Option<Arc<ProviderConnectionGuard>>,
    subscribers: RwLock<HashMap<SubscriberId, CancellationToken>>,
    broadcaster: tokio::sync::broadcast::Sender<Bytes>,
    stop_token: CancellationToken,
    burst_buffer: Arc<Mutex<BurstBuffer>>,
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
        // TODO channel size versus byte size,  channels are chunk sized, burst_buffer byte sized
        let burst_buffer_size_in_bytes = MIN_SHARED_BUFFER_SIZE.max(buf_size * 1024 * 12);
        Self {
            headers,
            buf_size,
            provider_guard,
            subscribers: RwLock::new(HashMap::new()),
            broadcaster,
            stop_token: CancellationToken::new(),
            burst_buffer : Arc::new(Mutex::new(BurstBuffer::new(burst_buffer_size_in_bytes))),
        }
    }

    async fn subscribe(&self, addr: &str, manager: Arc<SharedStreamManager>) -> (BoxedProviderStream, Option<String>) {
        let (client_tx, client_rx) = mpsc::channel(self.buf_size);
        let mut broadcast_rx = self.broadcaster.subscribe();
        let cancel_token = CancellationToken::new();
        self.subscribers.write().await.insert(addr.to_string(), cancel_token.clone());

        let address = addr.to_string();
        let client_tx_clone = client_tx.clone();
        let burst_buffer = self.burst_buffer.clone();

        tokio::spawn(async move {
            let snapshot = {
                let buffer = burst_buffer.lock().await;
                buffer.snapshot()
            };
            send_burst_buffer(&snapshot, &client_tx_clone, &cancel_token).await;

            let mut loop_cnt = 0;
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
                                loop_cnt += 1;
                                if loop_cnt >= YIELD_COUNTER {
                                   tokio::task::yield_now().await;
                                   loop_cnt = 0;
                                 }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                trace!("Client lagged behind. Skipped {skipped} messages. {address}");
                                loop_cnt += 1;
                               if loop_cnt >= YIELD_COUNTER {
                                   tokio::task::yield_now().await;
                                   loop_cnt = 0;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
            manager.release_connection(&address, false).await;
        });

        let provider = match &self.provider_guard {
            None => None,
            Some(connection_guard) => connection_guard.get_provider_name()
        };

        (convert_stream(ReceiverStream::new(client_rx).boxed()), provider)
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
        let burst_buffer = self.burst_buffer.clone();

        tokio::spawn(async move {
            let mut counter = 0usize;
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
                          {
                            let mut buffer = burst_buffer.lock().await;
                            buffer.push(&data);
                          }

                          match sender.send(data) {
                            Ok(clients) =>  {
                                if clients == 0 {
                                   debug_if_enabled!("No shared stream subscribers closing {}", sanitize_sensitive_info(&streaming_url));
                                   break;
                                }
                                counter += 1;
                                if counter >= YIELD_COUNTER {
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
                            tokio::task::yield_now().await;

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

    async fn unregister(&self, stream_url: &str, send_stop_signal: bool)
    {
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

    async fn subscribe_stream(&self, stream_url: &str, addr: Option<&str>, manager: Arc<SharedStreamManager>) -> Option<(BoxedProviderStream, Option<String>)> {
        let shared_stream_state = {
            self.shared_streams.read().await.get(stream_url).map(Arc::clone)
        };
        match shared_stream_state {
            None => None,
            Some(stream_state) => {
                if let Some(address) = addr {
                    debug_if_enabled!("Responding to existing shared client stream {} {}", address.to_string(),  sanitize_sensitive_info(stream_url));
                    self.shared_streams_by_addr.write().await.insert(address.to_string(), stream_url.to_owned());
                    Some(stream_state.subscribe(address, manager).await)
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
        provider_guard: Option<Arc<ProviderConnectionGuard>>) -> Option<(BoxedProviderStream, Option<String>)>
    where
        S: Stream<Item=Result<Bytes, E>> + Unpin + 'static + Send,
        E: std::fmt::Debug + Send,
    {
        let buf_size =  CHANNEL_SIZE.max(buffer_size);
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
    ) -> Option<(BoxedProviderStream, Option<String>)> {
        let manager = Arc::clone(&app_state.shared_stream_manager);
        app_state.shared_stream_manager.subscribe_stream(stream_url, addr, manager).await
    }
}


async fn send_burst_buffer(
    start_buffer: &VecDeque<Bytes>,
    client_tx: &Sender<Bytes>,
    cancellation_token: &CancellationToken) {
    for buf in start_buffer {
        if cancellation_token.is_cancelled() { return; }
        if let Err(err) = client_tx.send(buf.clone()).await {
            debug!("Error sending current chunk: {err}");
            return; // stop on send error
        }
    }
}