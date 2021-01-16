use std::{future::Future, marker::PhantomPinned, ops::DerefMut, pin::Pin};

use futures_task::{noop_waker, Context, Poll};
use pin_project::pin_project;

use self::{
    chain::ChainStream, filter::FilterStream, find::FindStream, map::MapStream, merge::MergeStream,
    once::OnceStream, repeat::RepeatStream,
};

mod chain;
mod errors;
mod filter;
mod find;
mod map;
mod merge;
mod once;
mod repeat;

#[cfg(feature = "logging")]
mod stream_log;

pub use errors::*;

/// An asynchronous stream, which produces a series of messages until closed.
#[must_use = "streams do nothing unless polled"]
pub trait Stream {
    type Item;

    /// Attempts to retrieve an item from the stream, without blocking.
    /// Returns `PollRecv::Ready(value)` if a message is ready
    /// Returns `PollRecv::Pending` if the stream is open, but no message is currently available.
    /// Returns `PollRecv::Closed` if the stream is closed, and no messages are expected.
    fn poll_recv(self: Pin<&mut Self>, cx: &mut Context<'_>) -> PollRecv<Self::Item>;

    /// Retrieves a message from the stream.
    /// Resolves to `Some(value)` if the stream is open
    /// Resolves to `None` if the stream is closed, and no further messages are expected.
    fn recv(&mut self) -> RecvFuture<'_, Self>
    where
        Self: Unpin,
    {
        RecvFuture::new(self)
    }

    /// Attempts to retrive a message from the stream, without blocking.
    /// Returns `Ok(value)` if a message was ready.
    /// Returns `TryRecvError::Pending` if the stream was open, but no messages were available.
    /// Returns `TryRecvError::Rejected` if the stream has been closed.
    fn try_recv(&mut self) -> Result<Self::Item, TryRecvError>
    where
        Self: Unpin,
    {
        let pin = Pin::new(self);
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);

        match pin.poll_recv(&mut context) {
            PollRecv::Ready(value) => Ok(value),
            PollRecv::Pending => Err(TryRecvError::Pending),
            PollRecv::Closed => Err(TryRecvError::Rejected),
        }
    }

    /// Returns a stream which produces a single value, and then is closed.
    fn once(item: Self::Item) -> OnceStream<Self::Item> {
        OnceStream::new(item)
    }

    /// Returns a stream which infiniately produces a clonable value.
    fn repeat(item: Self::Item) -> RepeatStream<Self::Item>
    where
        Self::Item: Clone,
    {
        RepeatStream::new(item)
    }

    /// Transforms the stream with a map function.
    fn map<Map, Into>(self, map: Map) -> MapStream<Self, Map, Into>
    where
        Map: Fn(Self::Item) -> Into,
        Self: Sized,
    {
        MapStream::new(self, map)
    }

    /// Filters messages returned by the stream, forwarding any to `.recv().await` where filter returns true.
    fn filter<Filter>(self, filter: Filter) -> FilterStream<Self, Filter>
    where
        Self: Sized + Unpin,
        Filter: FnMut(&Self::Item) -> bool + Unpin,
    {
        FilterStream::new(self, filter)
    }

    /// Merges two streams, returning values from both at once, until both are closed.
    fn merge<Other>(self, other: Other) -> MergeStream<Self, Other>
    where
        Other: Stream<Item = Self::Item>,
        Self: Sized,
    {
        MergeStream::new(self, other)
    }

    /// Chains two streams, returning values from this until it is closed, and then returning values from other.
    fn chain<Other>(self, other: Other) -> ChainStream<Self, Other>
    where
        Other: Stream<Item = Self::Item>,
        Self: Sized,
    {
        ChainStream::new(self, other)
    }

    /// Finds a message matching a condition.  When the condition is matched, a single value will be returned.
    fn find<Condition>(self, condition: Condition) -> FindStream<Self, Condition>
    where
        Self: Sized + Unpin,
        Condition: Fn(&Self::Item) -> bool + Unpin,
    {
        FindStream::new(self, condition)
    }

    /// Logs messages that are produced by the stream using the Debug trait, at the provided log level.
    ///
    /// Requires the `logging` feature
    #[cfg(feature = "logging")]
    fn log(self, level: log::Level) -> stream_log::StreamLog<Self>
    where
        Self: Sized,
        Self::Item: std::fmt::Debug,
    {
        stream_log::StreamLog::new(self, level)
    }
}

impl<S> Stream for &mut S
where
    S: Stream + Unpin + ?Sized,
{
    type Item = S::Item;

    fn poll_recv(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> PollRecv<Self::Item> {
        S::poll_recv(Pin::new(&mut **self), cx)
    }
}

impl<P, S> Stream for Pin<P>
where
    P: DerefMut<Target = S> + Unpin,
    S: Stream + Unpin + ?Sized,
{
    type Item = <S as Stream>::Item;

    fn poll_recv(self: Pin<&mut Self>, cx: &mut Context<'_>) -> PollRecv<Self::Item> {
        Pin::get_mut(self).as_mut().poll_recv(cx)
    }
}

/// An enum of poll responses that are produced by Stream implementations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollRecv<T> {
    /// An item is ready
    Ready(T),
    /// The channel is open, but no messages are ready and the receiver has registered with the waker context
    Pending,
    /// The channel is closed, and no messages will ever be delivered
    Closed,
}

#[pin_project]
#[must_use = "futures do nothing unless polled"]
pub struct RecvFuture<'s, S>
where
    S: Stream + ?Sized,
{
    recv: &'s mut S,
    #[pin]
    _pin: PhantomPinned,
}

impl<'s, S: Stream> RecvFuture<'s, S>
where
    S: ?Sized,
{
    pub fn new(recv: &'s mut S) -> RecvFuture<'s, S> {
        Self {
            recv,
            _pin: PhantomPinned,
        }
    }
}

impl<'s, S> Future for RecvFuture<'s, S>
where
    S: Stream + Unpin + ?Sized,
{
    type Output = Option<S::Item>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        match Pin::new(this.recv).poll_recv(cx) {
            PollRecv::Ready(v) => Poll::Ready(Some(v)),
            PollRecv::Pending => Poll::Pending,
            PollRecv::Closed => Poll::Ready(None),
        }
    }
}
