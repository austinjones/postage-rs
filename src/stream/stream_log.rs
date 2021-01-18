use crate::{Context, PollRecv, Stream};
use pin_project::pin_project;
use std::{fmt::Debug, pin::Pin};

use log::log;
#[pin_project]
pub struct StreamLog<S> {
    #[pin]
    stream: S,
    level: log::Level,
}

impl<S> StreamLog<S> {
    pub fn new(stream: S, level: log::Level) -> Self {
        StreamLog { stream, level }
    }
}

impl<S> Stream for StreamLog<S>
where
    S: Stream,
    S::Item: Debug,
{
    type Item = S::Item;

    fn poll_recv(self: Pin<&mut Self>, cx: &mut Context<'_>) -> crate::PollRecv<Self::Item> {
        let this = self.project();
        match this.stream.poll_recv(cx) {
            PollRecv::Ready(value) => {
                log!(*this.level, "{:?}", &value);
                PollRecv::Ready(value)
            }
            PollRecv::Pending => PollRecv::Pending,
            PollRecv::Closed => PollRecv::Closed,
        }
    }
}
