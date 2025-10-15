use crate::api::model::{StreamError, TransportStreamBuffer};
use bytes::Bytes;
use futures::Stream;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use std::future::Future;
use tokio::time::{Sleep, sleep};

pub struct CustomVideoStream {
    buffer: TransportStreamBuffer,
    delay: Option<Pin<Box<Sleep>>>,
    frame_interval: Duration,
}

impl CustomVideoStream {
    pub fn new(buffer: TransportStreamBuffer) -> Self {
        Self {
            buffer,
            delay: None,
            frame_interval: Duration::from_millis(40), // ca. 25 FPS
        }
    }
}

impl Stream for CustomVideoStream {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // is a timer active ?
        if let Some(ref mut delay) = self.delay {
            if delay.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
        }

        let chunk = self.buffer.next_chunk();

        // new sleep-timer
        self.delay = Some(Box::pin(sleep(self.frame_interval)));

        Poll::Ready(Some(Ok(chunk)))
    }
}
