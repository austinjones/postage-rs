use std::{sync::Arc, task::Waker};

use notifier::Notifier;
use ref_count::RefCount;

pub mod notifier;
mod oneshot_cell;
mod ref_count;
mod state_cell;
pub(crate) mod transfer;

pub(crate) fn shared<E>(extension: E) -> (SenderShared<E>, ReceiverShared<E>) {
    let inner = Arc::new(Shared::new(extension));

    let sender = SenderShared {
        inner: inner.clone(),
    };

    let receiver = ReceiverShared {
        inner: inner.clone(),
    };

    (sender, receiver)
}

pub struct Shared<E> {
    sender_notify: Notifier,
    sender_count: RefCount,
    receiver_notify: Notifier,
    receiver_count: RefCount,
    extension: E,
}

impl<E> Shared<E> {
    pub fn new(extension: E) -> Self {
        Self {
            sender_notify: Notifier::new(),
            sender_count: RefCount::new(1),
            receiver_notify: Notifier::new(),
            receiver_count: RefCount::new(1),
            extension,
        }
    }
}

pub struct SenderShared<E> {
    inner: Arc<Shared<E>>,
}

impl<E> SenderShared<E> {
    pub fn extension(&self) -> &E {
        &self.inner.extension
    }

    pub fn notify_receivers(&self) {
        self.inner.receiver_notify.notify();
    }

    pub fn subscribe_recv(&self, waker: Waker) {
        self.inner.sender_notify.subscribe(waker);
    }

    pub fn is_alive(&self) -> bool {
        self.inner.receiver_count.is_alive()
    }

    pub fn is_closed(&self) -> bool {
        !self.is_alive()
    }
}

impl<E> Clone for SenderShared<E> {
    fn clone(&self) -> Self {
        let inner = self.inner.clone();
        inner.sender_count.increment();

        Self { inner }
    }
}

impl<E> Drop for SenderShared<E> {
    fn drop(&mut self) {
        self.inner.sender_count.decrement()
    }
}

pub struct ReceiverShared<E> {
    inner: Arc<Shared<E>>,
}

impl<E> ReceiverShared<E> {
    pub fn extension(&self) -> &E {
        &self.inner.extension
    }

    pub fn notify_senders(&self) {
        self.inner.sender_notify.notify();
    }

    pub fn subscribe_send(&self, waker: Waker) {
        self.inner.receiver_notify.subscribe(waker);
    }

    pub fn is_alive(&self) -> bool {
        self.inner.sender_count.is_alive()
    }

    pub fn is_closed(&self) -> bool {
        !self.is_alive()
    }
}

impl<E> Clone for ReceiverShared<E> {
    fn clone(&self) -> Self {
        let inner = self.inner.clone();
        inner.receiver_count.increment();

        Self { inner }
    }
}

impl<E> Drop for ReceiverShared<E> {
    fn drop(&mut self) {
        self.inner.receiver_count.decrement()
    }
}
