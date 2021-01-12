use std::{future::Future, ops::DerefMut, pin::Pin, task::Poll};
use std::{marker::PhantomPinned, task::Context};

use futures_task::noop_waker;
use pin_project::pin_project;

mod chain;
mod errors;
mod filter;

#[cfg(feature = "logging")]
mod sink_log;

pub use errors::*;

pub trait Sink {
    type Item;

    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item>;

    fn send(&mut self, value: Self::Item) -> SendFuture<Self> {
        SendFuture::new(self, value)
    }

    fn try_send(&mut self, value: Self::Item) -> Result<(), TrySendError<Self::Item>>
    where
        Self: Unpin,
    {
        let pin = Pin::new(self);

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
        Self: Sized,
    {
        chain::SinkChain::new(before, self)
    }

    fn filter<Filter>(self, filter: Filter) -> filter::SinkFilter<Filter, Self>
    where
        Filter: FnMut(&Self::Item) -> bool,
        Self: Sized,
    {
        filter::SinkFilter::new(filter, self)
    }

    #[cfg(feature = "logging")]
    fn log(self, level: log::Level) -> sink_log::SinkLog<Self>
    where
        Self: Sized,
        Self::Item: std::fmt::Debug,
    {
        sink_log::SinkLog::new(self, level)
    }
}

impl<S> Sink for &mut S
where
    S: Sink + Unpin + ?Sized,
{
    type Item = S::Item;

    fn poll_send(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item> {
        S::poll_send(Pin::new(&mut **self), cx, value)
    }
}

impl<P, S> Sink for Pin<P>
where
    P: DerefMut<Target = S> + Unpin,
    S: Sink + Unpin + ?Sized,
{
    type Item = <S as Sink>::Item;

    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item> {
        Pin::get_mut(self).as_mut().poll_send(cx, value)
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
pub struct SendFuture<'s, S>
where
    S: Sink + ?Sized,
{
    #[pin]
    send: &'s mut S,
    value: Option<S::Item>,
    #[pin]
    _pin: PhantomPinned,
}

impl<'s, S> SendFuture<'s, S>
where
    S: Sink + ?Sized,
{
    pub fn new(send: &'s mut S, value: S::Item) -> SendFuture<S> {
        Self {
            send,
            value: Some(value),
            _pin: PhantomPinned,
        }
    }
}

impl<'s, S> Future for SendFuture<'s, S>
where
    S: Sink + Unpin + ?Sized,
{
    type Output = Result<(), SendError<S::Item>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let None = self.value {
            return Poll::Ready(Ok(()));
        }

        let this = self.project();

        match this.send.poll_send(cx, this.value.take().unwrap()) {
            crate::PollSend::Ready => Poll::Ready(Ok(())),
            crate::PollSend::Pending(value) => {
                *this.value = Some(value);
                Poll::Pending
            }
            crate::PollSend::Rejected(value) => Poll::Ready(Err(SendError(value))),
        }
    }
}
