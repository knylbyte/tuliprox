use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::fs::File;
use tokio::io::{
    self, AsyncRead, BufReader, AsyncSeekExt, AsyncReadExt, ReadBuf,
};
use async_compression::tokio::bufread::{GzipDecoder, ZlibDecoder};

use crate::utils::compression::compression_utils::{is_deflate, is_gzip};

pub struct CompressedFileReaderAsync {
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
}

impl CompressedFileReaderAsync {
    pub async fn new(path: &Path) -> std::io::Result<Self> {
        let file: File = tokio::fs::File::open(path).await?;

        let mut buffered_file = BufReader::new(file);
        let mut header = [0u8; 2];
        buffered_file.read_exact(&mut header).await?;
        buffered_file.seek(io::SeekFrom::Start(0)).await?;

        let reader: Box<dyn AsyncRead + Unpin + Send> = if is_gzip(&header) {
            Box::new(GzipDecoder::new(buffered_file))
        } else if is_deflate(&header) {
            Box::new(ZlibDecoder::new(buffered_file))
        } else {
            Box::new(buffered_file)
        };

        Ok(Self {
            reader: BufReader::new(reader),
        })
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

//
// impl AsyncBufRead for CompressedFileReaderAsync {
//     fn poll_fill_buf(
//         self: Pin<&mut Self>,
//         cx: &mut Context<'_>,
//     ) -> Poll<io::Result<&[u8]>> {
//         unsafe {
//             let this = self.get_unchecked_mut();
//             Pin::new_unchecked(&mut this.reader).poll_fill_buf(cx)
//         }
//     }
//
//     fn consume(mut self: Pin<&mut Self>, amt: usize) {
//         Pin::new(&mut self.reader).consume(amt)
//     }
// }
