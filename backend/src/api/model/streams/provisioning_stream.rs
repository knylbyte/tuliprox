use crate::api::model::{BoxedProviderStream, ConnectionManager, CustomVideoStreamType, ProviderHandle, StreamError};
use crate::utils::debug_if_enabled;
use bytes::Bytes;
use futures::{FutureExt, Stream};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::sync::oneshot;

#[derive(Debug, Copy, Clone)]
pub enum ProvisioningStreamKind {
    Provider,
    Custom(CustomVideoStreamType),
}

pub struct ProvisioningStreamPayload {
    pub stream: BoxedProviderStream,
    pub kind: ProvisioningStreamKind,
}

pub struct ProvisioningStream {
    loading_stream: BoxedProviderStream,
    provider_stream: Option<BoxedProviderStream>,
    provider_rx: Option<oneshot::Receiver<ProvisioningStreamPayload>>,
    cancel_flag: Arc<AtomicBool>,
    provider_handle: Arc<Mutex<Option<ProviderHandle>>>,
    connection_manager: Arc<ConnectionManager>,
    provider_stream_started: bool,
    provider_stream_kind: Option<ProvisioningStreamKind>,
}

impl ProvisioningStream {
    pub fn new(
        loading_stream: BoxedProviderStream,
        provider_rx: oneshot::Receiver<ProvisioningStreamPayload>,
        cancel_flag: Arc<AtomicBool>,
        provider_handle: Arc<Mutex<Option<ProviderHandle>>>,
        connection_manager: Arc<ConnectionManager>,
    ) -> Self {
        Self {
            loading_stream,
            provider_stream: None,
            provider_rx: Some(provider_rx),
            cancel_flag,
            provider_handle,
            connection_manager,
            provider_stream_started: false,
            provider_stream_kind: None,
        }
    }
}

impl Stream for ProvisioningStream {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.provider_stream.is_none() {
            if let Some(rx) = self.provider_rx.as_mut() {
                match rx.poll_unpin(cx) {
                    Poll::Ready(Ok(payload)) => {
                        match payload.kind {
                            ProvisioningStreamKind::Provider => {
                                debug_if_enabled!("Provisioning stream switching to provider stream");
                            }
                            ProvisioningStreamKind::Custom(custom_type) => {
                                debug_if_enabled!(
                                    "Provisioning stream switching to custom stream ({custom_type})"
                                );
                            }
                        }
                        self.provider_stream = Some(payload.stream);
                        self.provider_rx = None;
                        self.provider_stream_kind = Some(payload.kind);
                    }
                    Poll::Ready(Err(_)) => {
                        debug_if_enabled!("Provisioning stream stopped before provider stream was ready");
                        self.provider_rx = None;
                        return Poll::Ready(None);
                    }
                    Poll::Pending => {}
                }
            }
        }

        if let Some(stream) = self.provider_stream.as_mut() {
            let poll = Pin::new(stream).poll_next(cx);
            match poll {
                Poll::Ready(Some(Ok(bytes))) => {
                    if !self.provider_stream_started {
                        match self.provider_stream_kind {
                            Some(ProvisioningStreamKind::Provider) => {
                                debug_if_enabled!(
                                    "Provisioning provider stream first chunk ({} bytes)",
                                    bytes.len()
                                );
                            }
                            Some(ProvisioningStreamKind::Custom(custom_type)) => {
                                debug_if_enabled!(
                                    "Provisioning custom stream first chunk ({custom_type}, {} bytes)",
                                    bytes.len()
                                );
                            }
                            None => {
                                debug_if_enabled!(
                                    "Provisioning stream first chunk ({} bytes)",
                                    bytes.len()
                                );
                            }
                        }
                        self.provider_stream_started = true;
                    }
                    Poll::Ready(Some(Ok(bytes)))
                }
                Poll::Ready(Some(Err(err))) => {
                    debug_if_enabled!("Provisioning provider stream error: {err}");
                    Poll::Ready(Some(Err(err)))
                }
                Poll::Ready(None) => {
                    debug_if_enabled!("Provisioning provider stream ended");
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            }
        } else {
            Pin::new(&mut self.loading_stream).poll_next(cx)
        }
    }
}

impl Drop for ProvisioningStream {
    fn drop(&mut self) {
        self.cancel_flag.store(true, Ordering::Release);
        let provider_handle = self.provider_handle.lock().ok().and_then(|mut h| h.take());
        if let Some(handle) = provider_handle {
            let manager = Arc::clone(&self.connection_manager);
            tokio::spawn(async move {
                manager.release_provider_handle(Some(handle)).await;
            });
        }
    }
}
