use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicUsize, Ordering},
        RwLock, RwLockReadGuard,
    },
};

use static_assertions::{assert_impl_all, assert_not_impl_all};

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

assert_impl_all!(Sender<String>: Send);
assert_not_impl_all!(Sender<String>: Clone);

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
        self.shared.notify_receivers();

        PollSend::Ready
    }
}

pub struct Receiver<T> {
    pub(in crate::channels::watch) shared: ReceiverShared<StateExtension<T>>,
    pub(in crate::channels::watch) generation: AtomicUsize,
}

assert_impl_all!(Receiver<String>: Clone, Send);

impl<T> Stream for Receiver<T>
where
    T: Clone,
{
    type Item = T;

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        let stored_generation = self.shared.extension().generation(Ordering::Acquire);
        if self.generation.load(std::sync::atomic::Ordering::SeqCst) > stored_generation {
            if self.shared.is_closed() {
                return PollRecv::Closed;
            }

            self.shared.subscribe_send(cx.waker().clone());
            return PollRecv::Pending;
        }

        self.generation
            .store(stored_generation + 1, Ordering::Release);
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
pub struct Ref<'t, T> {
    pub(in crate::channels::watch) lock: RwLockReadGuard<'t, T>,
}

impl<'t, T> Deref for Ref<'t, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &*self.lock
    }
}

impl<T> Receiver<T> {
    pub fn borrow(&self) -> Ref<'_, T> {
        let lock = self.shared.extension().value.read().unwrap();
        Ref { lock }
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

    pub fn generation(&self, ordering: Ordering) -> usize {
        self.generation.load(ordering)
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use crate::{PollRecv, PollSend, Sink, Stream};
    use futures_test::task::{new_count_waker, noop_context, panic_context};

    use super::channel;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct State(usize);

    impl Default for State {
        fn default() -> Self {
            State(0)
        }
    }

    #[test]
    fn send_accepted() {
        let mut cx = noop_context();
        let (mut tx, _rx) = channel();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, State(1))
        );
        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, State(2))
        );
    }

    #[test]
    fn send_recv() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, State(1))
        );

        assert_eq!(
            PollRecv::Ready(State(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );
        assert_eq!(PollRecv::Pending, Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn recv_default() {
        let mut cx = panic_context();
        let (_tx, mut rx) = channel();

        assert_eq!(
            PollRecv::Ready(State(0)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );
        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut noop_context())
        );
    }

    #[test]
    fn borrow_default() {
        let (_tx, rx) = channel();

        assert_eq!(&State(0), &*rx.borrow());
    }

    #[test]
    fn borrow_sent() {
        let mut cx = panic_context();
        let (mut tx, rx) = channel();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, State(1))
        );

        assert_eq!(&State(1), &*rx.borrow());
    }

    #[test]
    fn sender_disconnect() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();
        let mut rx2 = rx.clone();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, State(1))
        );

        drop(tx);

        assert_eq!(
            PollRecv::Ready(State(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(PollRecv::Closed, Pin::new(&mut rx).poll_recv(&mut cx));

        assert_eq!(
            PollRecv::Ready(State(1)),
            Pin::new(&mut rx2).poll_recv(&mut cx)
        );

        assert_eq!(PollRecv::Closed, Pin::new(&mut rx2).poll_recv(&mut cx));
    }

    #[test]
    fn receiver_disconnect() {
        let mut cx = noop_context();
        let (mut tx, rx) = channel();

        drop(rx);

        assert_eq!(
            PollSend::Rejected(State(1)),
            Pin::new(&mut tx).poll_send(&mut cx, State(1))
        );
    }

    #[test]
    fn send_then_receiver_disconnect() {
        let mut cx = noop_context();
        let (mut tx, rx) = channel();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, State(1))
        );

        drop(rx);

        assert_eq!(
            PollSend::Rejected(State(2)),
            Pin::new(&mut tx).poll_send(&mut cx, State(2))
        );
    }

    #[test]
    fn wake_receiver() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel();

        let (w1, w1_count) = new_count_waker();
        let mut w1_context = Context::from_waker(&w1);

        assert_eq!(
            PollRecv::Ready(State(0)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );
        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w1_context)
        );

        assert_eq!(0, w1_count.get());

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, State(1))
        );

        assert_eq!(1, w1_count.get());

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, State(2))
        );

        assert_eq!(1, w1_count.get());
    }

    #[test]
    fn wake_receiver_on_disconnect() {
        let (tx, mut rx) = channel::<State>();

        let (w1, w1_count) = new_count_waker();
        let mut w1_context = Context::from_waker(&w1);

        assert_eq!(
            PollRecv::Ready(State(0)),
            Pin::new(&mut rx).poll_recv(&mut panic_context())
        );
        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w1_context)
        );

        assert_eq!(0, w1_count.get());

        drop(tx);

        assert_eq!(1, w1_count.get());
    }
}

#[cfg(test)]
mod tokio_tests {
    use crate::{
        test::{Channel, Message},
        Sink, Stream,
    };

    #[tokio::test]
    async fn simple() {
        let (mut tx, mut rx) = super::channel();

        tokio::task::spawn(async move {
            for message in Message::new_iter(0) {
                tx.send(message);
            }
        });

        let mut channel = Channel::new(0);
        while let Some(message) = rx.recv().await {
            channel.assert_message(&message);
        }
    }
}

#[cfg(test)]
mod async_std_tests {
    use async_std::task::spawn;

    use crate::{
        test::{Channel, Message},
        Sink, Stream,
    };

    #[async_std::test]
    async fn simple() {
        let (mut tx, mut rx) = super::channel();

        spawn(async move {
            for message in Message::new_iter(0) {
                tx.send(message);
            }
        });

        let mut channel = Channel::new(0);
        while let Some(message) = rx.recv().await {
            channel.assert_message(&message);
        }
    }
}
