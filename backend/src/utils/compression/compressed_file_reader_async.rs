use crate::utils::async_file_reader;
use crate::utils::compression::compression_utils::{is_deflate, is_gzip};
use async_compression::tokio::bufread::{GzipDecoder, ZlibDecoder};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::fs::File;
use tokio::io::{
    self, AsyncRead, AsyncReadExt, AsyncSeekExt, ReadBuf,
};

pub struct CompressedFileReaderAsync {
    reader: Box<dyn AsyncRead + Unpin + Send>,
}

impl CompressedFileReaderAsync {
    pub async fn new(path: &Path) -> std::io::Result<Self> {
        let file: File = tokio::fs::File::open(path).await?;

        let mut buffered_file = async_file_reader(file);
        let mut header = [0u8; 2];
        buffered_file.read_exact(&mut header).await?;
        buffered_file.seek(io::SeekFrom::Start(0)).await?;

        if is_gzip(&header) {
            Ok(Self { reader: Box::new(GzipDecoder::new(buffered_file)) })
        } else if is_deflate(&header) {
            Ok(Self { reader: Box::new(ZlibDecoder::new(buffered_file)) })
        } else {
            Ok(Self { reader: Box::new(buffered_file) })
        }
    }
}

impl AsyncRead for CompressedFileReaderAsync {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

