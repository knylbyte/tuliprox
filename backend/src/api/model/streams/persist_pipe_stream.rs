use std::path::Path;
use crate::api::model::StreamError;
use bytes::Bytes;
use log::{debug, error};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio_stream::{StreamExt};
use tokio_stream::wrappers::ReceiverStream;

pub fn tee_stream<S, W>(
    mut stream: S,
    mut writer: W,
    file_path: &Path,
    callback: Arc<dyn Fn(usize) + Send + Sync>,
) -> ReceiverStream<Result<Bytes, StreamError>>
where S: tokio_stream::Stream<Item = Result<Bytes, StreamError>> + Send + Unpin + 'static,
      W: tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, StreamError>>(32);
    let resource_path = file_path.to_owned();

    tokio::spawn(async move {
        let mut total_size = 0usize;
        let mut writer_active = true;
        let mut write_err: Option<StreamError> = None;

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if writer_active {
                        total_size += bytes.len();
                        if let Err(e) = writer.write_all(&bytes).await {
                            writer_active = false;
                            write_err = Some(StreamError::StdIo(e.to_string()));
                        }
                    }

                    let _ = tx.send(Ok(bytes)).await;
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                }
            }
        }

        // final flush & shutdown
        if writer_active {
            if let Err(e) = writer.flush().await {
                writer_active = false;
                write_err = Some(StreamError::StdIo(e.to_string()));
            }
        }
        let _ = writer.shutdown().await;

        if writer_active {
            debug!("Persisted {total_size} bytes to cache resource");
            (callback)(total_size);
        } else {
            if let Some(err) = write_err {
                error!("Persisted stream error: {err}.");
            }
            drop(writer);
            let _ = tokio::fs::remove_file(&resource_path).await;
        }
    });

    ReceiverStream::new(rx)
}

//
//
// /// `PersistPipeStream`
// ///
// /// Pipes bytes from an upstream stream to an async writer while tracking total size.
// /// Once the stream completes and the writer is flushed, the provided callback is invoked
// /// with the total number of bytes written.
// pub struct PersistPipeStream<S, W> {
//     inner: S,
//     completed: bool,
//     writer: W,
//     size: AtomicUsize,
//     callback: Arc<dyn Fn(usize) + Send + Sync>,
//     pending_writes: VecDeque<Bytes>,
//     current_offset: usize,
// }
//
// impl<S, W> PersistPipeStream<S, W>
// where
//     S: Stream + Unpin,
//     W: AsyncWrite + Unpin + 'static,
// {
//     pub fn new(inner: S, writer: W, callback: Arc<dyn Fn(usize) + Send + Sync>) -> Self {
//         Self {
//             inner,
//             completed: false,
//             writer,
//             size: AtomicUsize::new(0),
//             callback,
//             pending_writes: VecDeque::new(),
//             current_offset: 0,
//         }
//     }
//
//     fn enqueue_chunk(&mut self, bytes: Bytes) {
//         self.pending_writes.push_back(bytes);
//     }
//
//     fn poll_pending_writes(&mut self, cx: &mut Context<'_>) -> Poll<()> {
//         while let Some(chunk) = self.pending_writes.front() {
//             let chunk_len = chunk.len();
//             if self.current_offset >= chunk_len {
//                 if let Some(finished) = self.pending_writes.pop_front() {
//                     self.size.fetch_add(finished.len(), Ordering::AcqRel);
//                 }
//                 self.current_offset = 0;
//                 continue;
//             }
//
//             let remaining = &chunk[self.current_offset..];
//             match Pin::new(&mut self.writer).poll_write(cx, remaining) {
//                 Poll::Pending => return Poll::Pending,
//                 Poll::Ready(Ok(written)) => {
//                     if written == 0 {
//                         cx.waker().wake_by_ref();
//                         // Avoid tight loop if writer makes no progress.
//                         return Poll::Pending;
//                     }
//                     self.current_offset += written;
//                 }
//                 Poll::Ready(Err(err)) => {
//                     warn!(
//                         "Dropping {} buffered bytes after persistence write error: {err}",
//                         chunk_len.saturating_sub(self.current_offset)
//                     );
//                     error!("Error writing to resource file: {err}");
//                     self.pending_writes.pop_front();
//                     self.current_offset = 0;
//                 }
//             }
//         }
//
//         self.current_offset = 0;
//         Poll::Ready(())
//     }
//
//     fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<()> {
//         match Pin::new(&mut self.writer).poll_flush(cx) {
//             Poll::Pending => Poll::Pending,
//             Poll::Ready(Ok(())) => Poll::Ready(()),
//             Poll::Ready(Err(err)) => {
//                 error!("Error flushing resource file: {err}");
//                 Poll::Ready(())
//             }
//         }
//     }
//
//     fn finalize(&mut self) {
//         if !self.completed {
//             self.completed = true;
//             let size = self.size.load(Ordering::Acquire);
//             debug!("Persisted {size} bytes to cache resource");
//             (self.callback)(size);
//         }
//     }
// }
//
// impl<S, W> Stream for PersistPipeStream<S, W>
// where
//     S: Stream<Item=Result<bytes::Bytes, StreamError>> + Unpin,
//     W: AsyncWrite + Unpin + 'static,
// {
//     type Item = Result<Bytes, StreamError>;
//
//     fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         let this = self.get_mut();
//
//         if !this.pending_writes.is_empty() && this.poll_pending_writes(cx).is_pending() {
//             return Poll::Pending;
//         }
//
//         match Pin::new(&mut this.inner).poll_next(cx) {
//             Poll::Pending => Poll::Pending,
//             Poll::Ready(None) => {
//                 if !this.pending_writes.is_empty() {
//                     if this.poll_pending_writes(cx).is_pending() {
//                         return Poll::Pending;
//                     }
//                 }
//
//                 if !this.pending_writes.is_empty() {
//                     return Poll::Pending;
//                 }
//
//                 if this.poll_flush(cx).is_pending() {
//                     return Poll::Pending;
//                 }
//
//                 this.finalize();
//                 Poll::Ready(None)
//             }
//             Poll::Ready(Some(item)) => {
//                 if let Ok(bytes) = &item {
//                     this.enqueue_chunk(bytes.clone());
//                 }
//                 // Try to drain pending bytes after queuing new data.
//                 if this.poll_pending_writes(cx).is_pending() {
//                     // fall through: we still return the chunk to the caller even if persistence is pending
//                 }
//                 Poll::Ready(Some(item))
//             }
//         }
//     }
// }
