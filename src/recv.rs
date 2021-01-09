use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{PollRecv, Stream};

use pin_project::pin_project;

#[pin_project]
pub struct RecvFuture<R: Stream> {
    #[pin]
    recv: R,
}

impl<R: Stream> RecvFuture<R> {
    pub fn new(recv: R) -> RecvFuture<R> {
        Self { recv }
    }
}

impl<R: Stream> Future for RecvFuture<R> {
    type Output = Option<R::Item>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        match Stream::poll_recv(this.recv, cx) {
            PollRecv::Ready(v) => Poll::Ready(Some(v)),
            PollRecv::Pending => Poll::Pending,
            PollRecv::Closed => Poll::Ready(None),
        }
    }
}
