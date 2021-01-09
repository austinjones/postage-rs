use std::{future::Future, pin::Pin};

use futures_task::{noop_waker, Context, Poll};
use pin_project::pin_project;

use self::{
    chain::ChainStream, filter::FilterStream, find::FindStream, map::MapStream, merge::MergeStream,
    once::OnceStream, repeat::RepeatStream,
};

mod chain;
mod filter;
mod find;
mod map;
mod merge;
mod once;
mod repeat;

pub trait Stream: Sized {
    type Item;

    fn poll_recv(self: Pin<&mut Self>, cx: &mut Context<'_>) -> PollRecv<Self::Item>;

    fn recv(self) -> RecvFuture<Self> {
        RecvFuture::new(self)
    }

    fn try_recv(pin: Pin<&mut Self>) -> Result<Self::Item, TryRecvError> {
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);

        match pin.poll_recv(&mut context) {
            PollRecv::Ready(value) => Ok(value),
            PollRecv::Pending => Err(TryRecvError::Pending),
            PollRecv::Closed => Err(TryRecvError::Rejected),
        }
    }

    fn once(item: Self::Item) -> OnceStream<Self::Item> {
        OnceStream::new(item)
    }

    fn repeat(item: Self::Item) -> RepeatStream<Self::Item>
    where
        Self::Item: Clone,
    {
        RepeatStream::new(item)
    }

    fn map<Map, Into>(self, map: Map) -> MapStream<Self, Map, Into>
    where
        Map: Fn(Self::Item) -> Into,
    {
        MapStream::new(self, map)
    }

    fn filter<Filter>(self, filter: Filter) -> FilterStream<Self, Filter>
    where
        Self: Unpin,
        Filter: FnMut(&Self::Item) -> bool + Unpin,
    {
        FilterStream::new(self, filter)
    }

    fn merge<Other>(self, other: Other) -> MergeStream<Self, Other>
    where
        Other: Stream<Item = Self::Item>,
    {
        MergeStream::new(self, other)
    }

    fn chain<Other>(self, other: Other) -> ChainStream<Self, Other>
    where
        Other: Stream<Item = Self::Item>,
    {
        ChainStream::new(self, other)
    }

    fn find<Condition>(self, condition: Condition) -> FindStream<Self, Condition>
    where
        Self: Unpin,
        Condition: Fn(&Self::Item) -> bool + Unpin,
    {
        FindStream::new(self, condition)
    }

    // fn zip(self) {}

    // fn fold(self) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollRecv<T> {
    /// An item is ready
    Ready(T),
    /// The channel is open, but no messages are ready and the receiver has registered with the waker context
    Pending,
    /// The channel is closed, and no messages will ever be delivered
    Closed,
}

pub enum TryRecvError {
    /// The stream may produce an item at a later time
    Pending,
    /// The stream is closed, and will never produce an item
    Rejected,
}

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
