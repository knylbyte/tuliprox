use crate::api::model::{BoxedProviderStream, ConnectionManager, ProviderHandle, StreamError};
use crate::utils::debug_if_enabled;
use bytes::Bytes;
use futures::{FutureExt, Stream};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::sync::oneshot;

pub struct ProvisioningStream {
    loading_stream: BoxedProviderStream,
    provider_stream: Option<BoxedProviderStream>,
    provider_rx: Option<oneshot::Receiver<BoxedProviderStream>>,
    cancel_flag: Arc<AtomicBool>,
    provider_handle: Arc<Mutex<Option<ProviderHandle>>>,
    connection_manager: Arc<ConnectionManager>,
}

impl ProvisioningStream {
    pub fn new(
        loading_stream: BoxedProviderStream,
        provider_rx: oneshot::Receiver<BoxedProviderStream>,
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
        }
    }
}

impl Stream for ProvisioningStream {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.provider_stream.is_none() {
            if let Some(rx) = self.provider_rx.as_mut() {
                match rx.poll_unpin(cx) {
                    Poll::Ready(Ok(stream)) => {
                        debug_if_enabled!("Provisioning stream switching to provider stream");
                        self.provider_stream = Some(stream);
                        self.provider_rx = None;
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
            if let Poll::Ready(None) = poll {
                debug_if_enabled!("Provisioning provider stream ended");
            }
            poll
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
