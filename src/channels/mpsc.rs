use std::sync::{atomic::AtomicUsize, RwLock, RwLockReadGuard};

use crossbeam_queue::ArrayQueue;

use crate::{
    sync::{shared, ReceiverShared, SenderShared},
    PollRecv, PollSend, Sink, Stream,
};

pub fn channel<T: Clone + Default>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let (tx_shared, rx_shared) = shared(StateExtension::new(capacity));
    let sender = Sender { shared: tx_shared };

    let receiver = Receiver { shared: rx_shared };

    (sender, receiver)
}

pub struct Sender<T> {
    pub(in crate::channels::mpsc) shared: SenderShared<StateExtension<T>>,
}

impl<T> Sink for Sender<T> {
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        value: Self::Item,
    ) -> crate::PollSend<Self::Item> {
        if self.shared.is_closed() {
            return PollSend::Rejected(value);
        }

        let queue = &self.shared.extension().queue;

        match queue.push(value) {
            Ok(_) => {
                self.shared.notify_receivers();
                PollSend::Ready
            }
            Err(v) => {
                self.shared.subscribe_recv(cx.waker().clone());
                PollSend::Pending(v)
            }
        }
    }
}

pub struct Receiver<T> {
    pub(in crate::channels::mpsc) shared: ReceiverShared<StateExtension<T>>,
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

        match self.shared.extension().queue.pop() {
            Some(v) => {
                self.shared.notify_senders();
                PollRecv::Ready(v)
            }
            None => {
                self.shared.subscribe_send(cx.waker().clone());
                PollRecv::Pending
            }
        }
    }
}

struct StateExtension<T> {
    queue: ArrayQueue<T>,
}

impl<T> StateExtension<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: ArrayQueue::new(capacity),
        }
    }
}
