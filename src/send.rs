use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::Sink;
use pin_project::pin_project;

#[pin_project]
pub struct SendFuture<S: Sink> {
    #[pin]
    send: S,
    value: Option<S::Item>,
}

impl<S: Sink> SendFuture<S> {
    pub fn new(send: S, value: S::Item) -> SendFuture<S> {
        Self {
            send,
            value: Some(value),
        }
    }
}

impl<S: Sink> Future for SendFuture<S> {
    type Output = Result<(), S::Item>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let None = self.value {
            return Poll::Ready(Ok(()));
        }

        let this = self.project();

        match Sink::poll_send(this.send, cx, this.value.take().unwrap()) {
            crate::PollSend::Ready => Poll::Ready(Ok(())),
            crate::PollSend::Pending(value) => {
                *this.value = Some(value);
                Poll::Pending
            }
            crate::PollSend::Rejected(value) => Poll::Ready(Err(value)),
        }
    }
}
