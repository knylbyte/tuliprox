use crate::api::model::stream_error::StreamError;
use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;
use std::task::Poll;
use std::time::{Duration, Instant};
use crate::api::model::BoxedProviderStream;

pub struct TimedClientStream {
    inner: BoxedProviderStream,
    deadline: Instant,
}

impl TimedClientStream {
    pub(crate) fn new(inner: BoxedProviderStream, duration: u32) -> Self {
        let deadline = Instant::now() + Duration::from_secs(u64::from(duration));
        Self { inner, deadline }
    }
}
impl Stream for TimedClientStream {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>,cx: &mut std::task::Context<'_>,) -> Poll<Option<Self::Item>> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(None);
        }
        Pin::as_mut(&mut self.inner).poll_next(cx)
    }
}