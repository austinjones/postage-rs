use std::task::Context;
use std::{future::Future, pin::Pin, task::Poll};

use futures_task::noop_waker;
use pin_project::pin_project;

mod chain;
mod filter;

pub enum TrySendError<T> {
    /// The sink could accept the item at a later time
    Pending(T),
    /// The sink is closed, and will never accept the item
    Rejected(T),
}

pub trait Sink: Sized {
    type Item;

    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item>;

    fn send(self, value: Self::Item) -> SendFuture<Self> {
        SendFuture::new(self, value)
    }

    fn try_send(pin: Pin<&mut Self>, value: Self::Item) -> Result<(), TrySendError<Self::Item>> {
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);

        match pin.poll_send(&mut context, value) {
            PollSend::Ready => Ok(()),
            PollSend::Pending(value) => Err(TrySendError::Pending(value)),
            PollSend::Rejected(value) => Err(TrySendError::Rejected(value)),
        }
    }

    fn after<Before>(self, before: Before) -> chain::SinkChain<Before, Self>
    where
        Before: Sink<Item = Self::Item>,
    {
        chain::SinkChain::new(before, self)
    }

    fn filter<Filter>(self, filter: Filter) -> filter::SinkFilter<Filter, Self>
    where
        Filter: FnMut(&Self::Item) -> bool,
    {
        filter::SinkFilter::new(filter, self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollSend<T> {
    /// The item was accepted and sent
    Ready,
    /// The sender is pending, and has registered with the waker context
    Pending(T),
    /// The sender has been closed, and cannot send the item
    Rejected(T),
}

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
