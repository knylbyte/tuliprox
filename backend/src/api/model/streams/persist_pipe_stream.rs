use std::path::Path;
use crate::api::model::StreamError;
use bytes::Bytes;
use log::{debug, error};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio_stream::{StreamExt};
use tokio_stream::wrappers::ReceiverStream;

const FLUSH_INTERVAL: usize = 50;

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
        let mut write_counter = 0usize;

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if writer_active {
                        total_size += bytes.len();
                        if let Err(e) = writer.write_all(&bytes).await {
                            writer_active = false;
                            write_err = Some(StreamError::StdIo(e.to_string()));
                        } else {
                            write_counter += 1;
                            if write_counter > FLUSH_INTERVAL {
                                write_counter = 0;
                                if let Err(err) = writer.flush().await {
                                    write_err = Some(StreamError::StdIo(format!("Failed periodic flush of tee_stream writer {err}")));
                                }
                            }
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
