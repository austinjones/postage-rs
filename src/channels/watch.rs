use std::sync::{
    atomic::{AtomicUsize, Ordering},
    RwLock,
};

use crate::{
    sync::{shared, ReceiverShared, SenderShared},
    PollRecv, PollSend, Sink, Stream,
};

pub fn channel<T: Clone + Default>() -> (Sender<T>, Receiver<T>) {
    let (tx_shared, rx_shared) = shared(StateExtension::new(T::default()));
    let sender = Sender { shared: tx_shared };

    let receiver = Receiver {
        shared: rx_shared,
        generation: AtomicUsize::new(0),
    };

    (sender, receiver)
}

pub struct Sender<T> {
    pub(in crate::channels::watch) shared: SenderShared<StateExtension<T>>,
}

impl<T> Sink for Sender<T> {
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        value: Self::Item,
    ) -> crate::PollSend<Self::Item> {
        if self.shared.is_closed() {
            return PollSend::Rejected(value);
        }

        self.shared.extension().push(value);
        PollSend::Ready
    }
}

pub struct Receiver<T> {
    pub(in crate::channels::watch) shared: ReceiverShared<StateExtension<T>>,
    pub(in crate::channels::watch) generation: AtomicUsize,
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
        if self.shared.is_closed() {
            return PollRecv::Closed;
        }

        if self.shared.extension().generation()
            < self.generation.load(std::sync::atomic::Ordering::SeqCst)
        {
            self.shared.subscribe_send(cx.waker().clone());
            return PollRecv::Pending;
        }

        let borrow = self.shared.extension().value.read().unwrap();
        PollRecv::Ready(borrow.clone())
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            generation: AtomicUsize::new(0),
        }
    }
}

struct StateExtension<T> {
    generation: AtomicUsize,
    value: RwLock<T>,
}

impl<T> StateExtension<T> {
    pub fn new(value: T) -> Self {
        Self {
            generation: AtomicUsize::new(0),
            value: RwLock::new(value),
        }
    }

    pub fn push(&self, value: T) {
        let mut lock = self.value.write().unwrap();
        *lock = value;
        drop(lock);

        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn generation(&self) -> usize {
        self.generation.load(Ordering::SeqCst)
    }
}
