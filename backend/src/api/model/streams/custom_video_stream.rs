use crate::api::model::StreamError;
use crate::api::model::TransportStreamBuffer;
use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};


pub struct CustomVideoStream {
    buffer: TransportStreamBuffer,
}

impl CustomVideoStream {
    pub fn new(buffer: TransportStreamBuffer) -> Self {
        Self {
            buffer
        }
    }
}

impl Stream for CustomVideoStream {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>,) -> Poll<Option<Self::Item>> {
        Poll::Ready(Some(Ok(self.buffer.next_chunk())))
    }
}
