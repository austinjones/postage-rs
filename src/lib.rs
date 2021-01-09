mod channels;
mod recv;
mod send;
mod sync;
mod try_recv;
mod try_send;

use recv::RecvFuture;
use send::SendFuture;

use std::{pin::Pin, task::Context};

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

    fn map(self) {}

    fn filter(self) {}

    fn split(self) {}

    fn chain(self) {}
}

pub enum PollSend<T> {
    /// The item was accepted and sent
    Ready,
    /// The sender is pending, and has registered with the waker context
    Pending(T),
    /// The sender has been closed, and cannot send the item
    Rejected(T),
}

pub trait Stream: Sized {
    type Item;

    fn poll_recv(self: Pin<&mut Self>, cx: &mut Context<'_>) -> PollRecv<Self::Item>;

    fn recv(self) -> RecvFuture<Self> {
        RecvFuture::new(self)
    }

    fn map(self) {}

    fn filter(self) {}

    fn merge(self) {}

    fn chain(self) {}

    fn zip(self) {}

    fn fold(self) {}

    fn find(self) {}
}

pub enum PollRecv<T> {
    /// An item is ready
    Ready(T),
    /// The channel is open, but no messages are ready and the receiver has registered with the waker context
    Pending,
    /// The channel is closed, and no messages will ever be delivered
    Closed,
}
