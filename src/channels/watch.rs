//! A state distribution channel.  The internal state can be borrowed or cloned, but receivers do not observe every value.
//!  
//! When the channel is created, the receiver will immediately observe `T::default()`.  Cloned receivers will immediately observe the latest stored value.
//!
//! Receivers can also borrow the latest value.

use super::SendSyncMessage;
use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicUsize, Ordering},
        RwLock, RwLockReadGuard,
    },
};

use static_assertions::{assert_impl_all, assert_not_impl_all};

use crate::{
    sink::{PollSend, Sink},
    stream::{PollRecv, Stream},
    sync::{shared, ReceiverShared, SenderShared},
};

/// Constructs a new watch channel pair, filled with T::default()
pub fn channel<T: Clone + Default>() -> (Sender<T>, Receiver<T>) {
    #[cfg(feature = "debug")]
    log::error!("Creating watch channel");

    let (tx_shared, rx_shared) = shared(StateExtension::new(T::default()));
    let sender = Sender { shared: tx_shared };

    let receiver = Receiver {
        shared: rx_shared,
        generation: AtomicUsize::new(0),
    };

    (sender, receiver)
}

/// The sender half of a watch channel.  The stored value can be updated with the postage::Sink trait.
pub struct Sender<T> {
    pub(in crate::channels::watch) shared: SenderShared<StateExtension<T>>,
}

assert_impl_all!(Sender<SendSyncMessage>: Send, Sync);
assert_not_impl_all!(Sender<SendSyncMessage>: Clone);

impl<T> Sink for Sender<T> {
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut crate::Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item> {
        if self.shared.is_closed() {
            return PollSend::Rejected(value);
        }

        self.shared.extension().push(value);
        self.shared.notify_receivers();

        PollSend::Ready
    }
}

/// The receiver half of a watch channel.  Can recieve state updates with the postage::Sink trait.
///
/// The reciever will be woken when new values arive, but is not guaranteed to recieve every message.
pub struct Receiver<T> {
    pub(in crate::channels::watch) shared: ReceiverShared<StateExtension<T>>,
    pub(in crate::channels::watch) generation: AtomicUsize,
}

assert_impl_all!(Receiver<SendSyncMessage>: Clone, Send, Sync);

impl<T> Stream for Receiver<T>
where
    T: Clone,
{
    type Item = T;

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut crate::Context<'_>,
    ) -> PollRecv<Self::Item> {
        match self.try_recv_internal() {
            TryRecv::Pending => {
                self.shared.subscribe_send(cx);

                match self.try_recv_internal() {
                    TryRecv::Pending => {
                        if self.shared.is_closed() {
                            return PollRecv::Closed;
                        }

                        PollRecv::Pending
                    }
                    TryRecv::Ready(v) => PollRecv::Ready(v),
                }
            }
            TryRecv::Ready(v) => PollRecv::Ready(v),
        }
    }
}

impl<T> Receiver<T>
where
    T: Clone,
{
    fn try_recv_internal(&self) -> TryRecv<T> {
        let state = self.shared.extension();
        if self.generation.load(std::sync::atomic::Ordering::SeqCst)
            > state.generation(Ordering::SeqCst)
        {
            return TryRecv::Pending;
        }

        let borrow = self.shared.extension().value.read().unwrap();
        let stored_generation = self.shared.extension().generation(Ordering::SeqCst);
        self.generation
            .store(stored_generation + 1, Ordering::Release);
        TryRecv::Ready(borrow.clone())
    }
}

enum TryRecv<T> {
    Pending,
    Ready(T),
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

        self.generation.fetch_add(1, Ordering::SeqCst);
        drop(lock);
    }

    pub fn generation(&self, ordering: Ordering) -> usize {
        self.generation.load(ordering)
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use super::channel;
    use crate::{
        sink::{PollSend, Sink},
        stream::{PollRecv, Stream},
        test::{noop_context, panic_context},
    };
    use futures_test::task::new_count_waker;

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
        let w1_context = Context::from_waker(&w1);

        assert_eq!(
            PollRecv::Ready(State(0)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );
        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w1_context.into())
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
        let w1_context = Context::from_waker(&w1);

        assert_eq!(
            PollRecv::Ready(State(0)),
            Pin::new(&mut rx).poll_recv(&mut panic_context())
        );
        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w1_context.into())
        );

        assert_eq!(0, w1_count.get());

        drop(tx);

        assert_eq!(1, w1_count.get());
    }
}

#[cfg(test)]
mod tokio_tests {
    use tokio::{spawn, time::timeout};

    use crate::{
        sink::Sink,
        stream::Stream,
        test::{Channel, Channels, Message, CHANNEL_TEST_RECEIVERS, TEST_TIMEOUT},
    };

    #[tokio::test]
    async fn simple() {
        let (mut tx, mut rx) = super::channel();

        tokio::task::spawn(async move {
            let mut iter = Message::new_iter(0);
            // skip state 0
            iter.next();
            for message in iter {
                tx.send(message).await.expect("send failed");
            }
        });

        timeout(TEST_TIMEOUT, async move {
            let mut channel = Channel::new(0).allow_skips();
            while let Some(message) = rx.recv().await {
                channel.assert_message(&message);
            }
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn multi_receiver() {
        let (mut tx, rx) = super::channel();

        tokio::task::spawn(async move {
            let mut iter = Message::new_iter(0);
            // skip state 0
            iter.next();
            for message in iter {
                tx.send(message).await.expect("send failed");
            }
        });

        let handles = (0..CHANNEL_TEST_RECEIVERS).map(move |_| {
            let mut rx2 = rx.clone();
            let mut channels = Channels::new(CHANNEL_TEST_RECEIVERS).allow_skips();

            spawn(async move {
                while let Some(message) = rx2.recv().await {
                    channels.assert_message(&message);
                }
            })
        });

        timeout(TEST_TIMEOUT, async move {
            for handle in handles {
                handle.await.expect("join failed");
            }
        })
        .await
        .expect("test timeout");
    }
}

#[cfg(test)]
mod async_std_tests {
    use async_std::{future::timeout, task::spawn};

    use crate::{
        sink::Sink,
        stream::Stream,
        test::{Channel, Channels, Message, CHANNEL_TEST_RECEIVERS, TEST_TIMEOUT},
    };

    #[async_std::test]
    async fn simple() {
        let (mut tx, mut rx) = super::channel();

        spawn(async move {
            let mut iter = Message::new_iter(0);
            // skip state 0
            iter.next();
            for message in iter {
                tx.send(message).await.expect("send failed");
            }
        });

        timeout(TEST_TIMEOUT, async move {
            let mut channel = Channel::new(0).allow_skips();
            while let Some(message) = rx.recv().await {
                channel.assert_message(&message);
            }
        })
        .await
        .expect("test timeout");
    }

    #[tokio::test]
    async fn multi_receiver() {
        let (mut tx, rx) = super::channel();

        tokio::task::spawn(async move {
            let mut iter = Message::new_iter(0);
            // skip state 0
            iter.next();
            for message in iter {
                tx.send(message).await.expect("send failed");
            }
        });

        let handles = (0..CHANNEL_TEST_RECEIVERS).map(move |_| {
            let mut rx2 = rx.clone();
            let mut channels = Channels::new(CHANNEL_TEST_RECEIVERS).allow_skips();

            spawn(async move {
                while let Some(message) = rx2.recv().await {
                    channels.assert_message(&message);
                }
            })
        });

        timeout(TEST_TIMEOUT, async move {
            for handle in handles {
                handle.await;
            }
        })
        .await
        .expect("test timeout");
    }
}
