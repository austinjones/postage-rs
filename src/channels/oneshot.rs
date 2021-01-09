use std::sync::Arc;

use crate::{sync::transfer::Transfer, PollSend, Sink, Stream};

pub fn channel<T: Clone + Default>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Transfer::new());
    let sender = Sender {
        shared: shared.clone(),
    };

    let receiver = Receiver { shared };

    (sender, receiver)
}

pub struct Sender<T> {
    pub(in crate::channels::oneshot) shared: Arc<Transfer<T>>,
}

impl<T> Sink for Sender<T> {
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        value: Self::Item,
    ) -> crate::PollSend<Self::Item> {
        match self.shared.send(value) {
            Ok(_) => PollSend::Ready,
            Err(v) => PollSend::Rejected(v),
        }
    }
}

pub struct Receiver<T> {
    pub(in crate::channels::oneshot) shared: Arc<Transfer<T>>,
}

impl<T> Stream for Receiver<T>
where
    T: Clone,
{
    type Item = T;

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        self.shared.recv(cx.waker())
    }
}
