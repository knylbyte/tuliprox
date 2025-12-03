use crate::api::model::StreamError;
use bytes::Bytes;
use log::{debug, error, warn};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;
use tokio_stream::Stream;

/// `PersistPipeStream`
///
/// Pipes bytes from an upstream stream to an async writer while tracking total size.
/// Once the stream completes and the writer is flushed, the provided callback is invoked
/// with the total number of bytes written.
pub struct PersistPipeStream<S, W> {
    inner: S,
    completed: bool,
    writer: W,
    size: AtomicUsize,
    callback: Arc<dyn Fn(usize) + Send + Sync>,
    pending_writes: VecDeque<Bytes>,
    current_offset: usize,
}

impl<S, W> PersistPipeStream<S, W>
where
    S: Stream + Unpin,
    W: AsyncWrite + Unpin + 'static,
{
    pub fn new(inner: S, writer: W, callback: Arc<dyn Fn(usize) + Send + Sync>) -> Self {
        Self {
            inner,
            completed: false,
            writer,
            size: AtomicUsize::new(0),
            callback,
            pending_writes: VecDeque::new(),
            current_offset: 0,
        }
    }

    fn enqueue_chunk(&mut self, bytes: Bytes) {
        self.pending_writes.push_back(bytes);
    }

    fn poll_pending_writes(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        while let Some(chunk) = self.pending_writes.front() {
            let chunk_len = chunk.len();
            if self.current_offset >= chunk_len {
                if let Some(finished) = self.pending_writes.pop_front() {
                    self.size.fetch_add(finished.len(), Ordering::AcqRel);
                }
                self.current_offset = 0;
                continue;
            }

            let remaining = &chunk[self.current_offset..];
            match Pin::new(&mut self.writer).poll_write(cx, remaining) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(written)) => {
                    if written == 0 {
                        // Avoid tight loop if writer makes no progress.
                        return Poll::Pending;
                    }
                    self.current_offset += written;
                }
                Poll::Ready(Err(err)) => {
                    warn!(
                        "Dropping {} buffered bytes after persistence write error: {err}",
                        chunk_len.saturating_sub(self.current_offset)
                    );
                    error!("Error writing to resource file: {err}");
                    self.pending_writes.pop_front();
                    self.current_offset = 0;
                }
            }
        }

        self.current_offset = 0;
        Poll::Ready(())
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match Pin::new(&mut self.writer).poll_flush(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(()),
            Poll::Ready(Err(err)) => {
                error!("Error flushing resource file: {err}");
                Poll::Ready(())
            }
        }
    }

    fn finalize(&mut self) {
        if !self.completed {
            self.completed = true;
            let size = self.size.load(Ordering::Acquire);
            debug!("Persisted {size} bytes to cache resource");
            (self.callback)(size);
        }
    }
}

impl<S, W> Stream for PersistPipeStream<S, W>
where
    S: Stream<Item=Result<bytes::Bytes, StreamError>> + Unpin,
    W: AsyncWrite + Unpin + 'static,
{
    type Item = Result<Bytes, StreamError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if !this.pending_writes.is_empty() && this.poll_pending_writes(cx).is_pending() {
            return Poll::Pending;
        }

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => {
                if this.poll_pending_writes(cx).is_pending() {
                    return Poll::Pending;
                }
                if this.poll_flush(cx).is_pending() {
                    return Poll::Pending;
                }
                this.finalize();
                Poll::Ready(None)
            }
            Poll::Ready(Some(item)) => {
                if let Ok(bytes) = &item {
                    this.enqueue_chunk(bytes.clone());
                }
                // Try to drain pending bytes after queuing new data.
                if this.poll_pending_writes(cx).is_pending() {
                    // fall through: we still return the chunk to the caller even if persistence is pending
                }
                Poll::Ready(Some(item))
            }
        }
    }
}
