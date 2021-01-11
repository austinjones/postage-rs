use std::{future::Future, ops::DerefMut, pin::Pin, task::Poll};
use std::{marker::PhantomPinned, task::Context};

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

impl<S> Sink for &mut S
where
    S: Sink + Unpin,
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
    S: Sink + Unpin,
{
    type Item = <S as Sink>::Item;

    fn poll_send(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item> {
        let this: &mut S = &mut *self.as_mut();
        Pin::new(this).poll_send(cx, value)
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

#[derive(Debug, PartialEq, Eq)]
pub struct SendError<T>(pub T);

impl<T> std::fmt::Display for SendError<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SendError: ")?;

        f.write_fmt(format_args!("{:?}", &self.0))?;

        Ok(())
    }
}

impl<T> std::error::Error for SendError<T> where T: std::fmt::Debug {}

#[pin_project]
pub struct SendFuture<'s, S: Sink> {
    send: &'s mut S,
    value: Option<S::Item>,
    #[pin]
    _pin: PhantomPinned,
}

impl<'s, S: Sink> SendFuture<'s, S> {
    pub fn new(send: &'s mut S, value: S::Item) -> SendFuture<S> {
        Self {
            send,
            value: Some(value),
            _pin: PhantomPinned,
        }
    }
}

impl<'s, S: Sink + Unpin> Future for SendFuture<'s, S> {
    type Output = Result<(), SendError<S::Item>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let None = self.value {
            return Poll::Ready(Ok(()));
        }

        let this = self.project();

        match Pin::new(this.send).poll_send(cx, this.value.take().unwrap()) {
            crate::PollSend::Ready => Poll::Ready(Ok(())),
            crate::PollSend::Pending(value) => {
                *this.value = Some(value);
                Poll::Pending
            }
            crate::PollSend::Rejected(value) => Poll::Ready(Err(SendError(value))),
        }
    }
}
