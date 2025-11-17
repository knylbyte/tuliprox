use futures::{stream::Stream, task::{Context, Poll}, StreamExt};
use std::{
    pin::Pin,
    sync::Arc,
};
use std::cmp::{max};
use log::{debug, warn};
use tokio::sync::mpsc::{channel, Sender};
use tokio_stream::wrappers::ReceiverStream;
use crate::api::model::{BoxedProviderStream};
use crate::api::model::StreamError;
use crate::tools::atomic_once_flag::AtomicOnceFlag;

pub const CHANNEL_SIZE: usize = 1024;

pub(in crate::api::model) struct BufferedStream {
    stream: ReceiverStream<Result<bytes::Bytes, StreamError>>,
    close_signal: Arc<AtomicOnceFlag>
}

impl BufferedStream {
    pub fn new(stream: BoxedProviderStream, buffer_size: usize, client_close_signal: Arc<AtomicOnceFlag>, _url: &str) -> Self {
        // TODO make channel_size  based on bytes not entries
        let (tx, rx) = channel(max(buffer_size, CHANNEL_SIZE));
        tokio::spawn(Self::buffer_stream(tx, stream, Arc::clone(&client_close_signal)));
        Self {
            stream: ReceiverStream::new(rx),
            close_signal: client_close_signal,
        }
    }

    async fn buffer_stream(
        tx: Sender<Result<bytes::Bytes, StreamError>>,
        mut stream: BoxedProviderStream,
        client_close_signal: Arc<AtomicOnceFlag>,
    ) {
        while client_close_signal.is_active() {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    let chunk_len = chunk.len();
                    if tx.send(Ok(chunk)).await.is_err() {
                        warn!(
                            "Buffered stream channel closed before delivering {} bytes to client",
                            chunk_len
                        );
                        client_close_signal.notify();
                        break;
                    }
                }
                Some(Err(err)) => {
                    let err_msg = err.to_string();
                    if tx.send(Err(err)).await.is_err() {
                        warn!(
                            "Buffered stream dropped stream error due to closed receiver: {err_msg}"
                        );
                        client_close_signal.notify();
                    }
                    break;
                }
                None => {
                    debug!("Upstream provider completed buffered stream");
                    break;
                }
            }
        }
        if !client_close_signal.is_active() {
            debug!("Client close signal fired; buffered stream exiting");
        }
        drop(tx);
    }
}

impl Stream for BufferedStream {
    type Item = Result<bytes::Bytes, StreamError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.close_signal.is_active() {
            Pin::new(&mut self.get_mut().stream).poll_next(cx)
        } else {
            Poll::Ready(None)
        }
    }
}
