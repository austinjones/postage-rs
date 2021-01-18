//! Barrier channels can be used to synchronize events, but do not transmit any data.  
//! When the sender is dropped (or `tx.send(())` is called), the receiver is awoken.  
//! This can be used to asynchronously coordinate actions between tasks.

use std::sync::Arc;

use atomic::{Atomic, Ordering};
use static_assertions::{assert_impl_all, assert_not_impl_all};

use crate::{sync::notifier::Notifier, PollRecv, PollSend, Sink, Stream};

/// Constructs a pair of barrier endpoints
pub fn channel() -> (Sender, Receiver) {
    #[cfg(feature = "debug")]
    log::error!("Creating barrier channel");
    let shared = Arc::new(Shared {
        state: Atomic::new(State::Pending),
        notify_rx: Notifier::new(),
    });

    let sender = Sender {
        shared: shared.clone(),
    };

    let receiver = Receiver { shared };

    (sender, receiver)
}

/// The sender half of a barrier channel.  Dropping the sender transmits to the receiver.
///
/// Can also be triggered by sending `()` with the postage::Sink trait.
pub struct Sender {
    pub(in crate::channels::barrier) shared: Arc<Shared>,
}

assert_impl_all!(Sender: Send);
assert_not_impl_all!(Sender: Clone);

impl Sink for Sender {
    type Item = ();

    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut crate::Context<'_>,
        _value: (),
    ) -> crate::PollSend<Self::Item> {
        match self.shared.state.load(Ordering::Acquire) {
            State::Pending => {
                self.shared.close();
                PollSend::Ready
            }
            State::Closed => PollSend::Rejected(()),
        }
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.shared.close();
    }
}

/// A barrier reciever.  Can be used with the postage::Stream trait to return a `()` value when the Sender is dropped.
#[derive(Clone)]
pub struct Receiver {
    pub(in crate::channels::barrier) shared: Arc<Shared>,
}

assert_impl_all!(Receiver: Send, Clone);

#[derive(Copy, Clone)]
enum State {
    Pending,
    Closed,
}

struct Shared {
    state: Atomic<State>,
    notify_rx: Notifier,
}

impl Shared {
    pub fn close(&self) {
        self.state.store(State::Closed, Ordering::Release);
        self.notify_rx.notify();
    }
}

impl Stream for Receiver {
    type Item = ();

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut crate::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        match self.shared.state.load(Ordering::Acquire) {
            State::Pending => {
                self.shared.notify_rx.subscribe(cx);
                PollRecv::Pending
            }
            State::Closed => PollRecv::Ready(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use crate::test::{noop_context, panic_context};
    use crate::{PollRecv, PollSend, Sink, Stream};
    use futures_test::task::new_count_waker;

    use super::channel;

    #[test]
    fn send_accepted() {
        let mut cx = noop_context();
        let (mut tx, _rx) = channel();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));
        assert_eq!(
            PollSend::Rejected(()),
            Pin::new(&mut tx).poll_send(&mut cx, ())
        );
    }

    #[test]
    fn send_recv() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn sender_disconnect() {
        let mut cx = noop_context();
        let (tx, mut rx) = channel();

        drop(tx);

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn send_then_disconnect() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        drop(tx);

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn receiver_disconnect() {
        let mut cx = noop_context();
        let (mut tx, rx) = channel();

        drop(rx);

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));
    }

    #[test]
    fn receiver_clone() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();
        let mut rx2 = rx.clone();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx2).poll_recv(&mut cx));
    }

    #[test]
    fn receiver_send_then_clone() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        let mut rx2 = rx.clone();

        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx).poll_recv(&mut cx));
        assert_eq!(PollRecv::Ready(()), Pin::new(&mut rx2).poll_recv(&mut cx));
    }

    #[test]
    fn wake_receiver() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel();

        let (w, w_count) = new_count_waker();
        let w_context = Context::from_waker(&w);

        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w_context.into())
        );

        assert_eq!(0, w_count.get());

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        assert_eq!(1, w_count.get());

        assert_eq!(
            PollSend::Rejected(()),
            Pin::new(&mut tx).poll_send(&mut cx, ())
        );

        assert_eq!(1, w_count.get());
    }

    #[test]
    fn wake_receiver_on_disconnect() {
        let (tx, mut rx) = channel();

        let (w1, w1_count) = new_count_waker();
        let w1_context = Context::from_waker(&w1);

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
    use std::time::Duration;

    use tokio::{task::spawn, time::timeout};

    use crate::{
        test::{CHANNEL_TEST_ITERATIONS, CHANNEL_TEST_RECEIVERS, TEST_TIMEOUT},
        Sink, Stream,
    };

    use super::Receiver;

    async fn assert_rx(mut rx: Receiver) {
        if let Err(_e) = timeout(Duration::from_millis(100), rx.recv()).await {
            panic!("Timeout waiting for barrier");
        }
    }

    #[tokio::test]
    async fn simple() {
        for _ in 0..CHANNEL_TEST_ITERATIONS {
            let (mut tx, rx) = super::channel();

            tx.send(()).await.expect("Should send message");

            assert_rx(rx).await;
        }
    }

    #[tokio::test]
    async fn simple_drop() {
        for _ in 0..CHANNEL_TEST_ITERATIONS {
            let (tx, rx) = super::channel();

            drop(tx);

            assert_rx(rx).await;
        }
    }

    #[tokio::test]
    async fn multi_receiver() {
        for _ in 0..CHANNEL_TEST_ITERATIONS {
            let (tx, rx) = super::channel();

            let handles = (0..CHANNEL_TEST_RECEIVERS).map(move |_| {
                let rx2 = rx.clone();

                spawn(async move {
                    assert_rx(rx2).await;
                })
            });

            drop(tx);

            let rx_handle = spawn(async move {
                for handle in handles {
                    handle.await.expect("Assertion failure");
                }
            });

            timeout(TEST_TIMEOUT, rx_handle)
                .await
                .expect("test timeout")
                .expect("join error");
        }
    }
}

#[cfg(test)]
mod async_std_tests {
    use std::time::Duration;

    use async_std::{future::timeout, task::spawn};

    use crate::{
        test::{CHANNEL_TEST_ITERATIONS, CHANNEL_TEST_RECEIVERS, TEST_TIMEOUT},
        Sink, Stream,
    };

    use super::Receiver;

    async fn assert_rx(mut rx: Receiver) {
        if let Err(_e) = timeout(Duration::from_millis(100), rx.recv()).await {
            panic!("Timeout waiting for barrier");
        }
    }

    #[async_std::test]
    async fn simple() {
        for _ in 0..CHANNEL_TEST_ITERATIONS {
            let (mut tx, rx) = super::channel();

            tx.send(()).await.expect("Should send message");

            timeout(TEST_TIMEOUT, async move {
                assert_rx(rx).await;
            })
            .await
            .expect("test timeout");
        }
    }

    #[async_std::test]
    async fn simple_drop() {
        for _ in 0..CHANNEL_TEST_ITERATIONS {
            let (tx, rx) = super::channel();

            drop(tx);

            timeout(TEST_TIMEOUT, async move {
                assert_rx(rx).await;
            })
            .await
            .expect("test timeout");
        }
    }

    #[async_std::test]
    async fn multi_receiver() {
        for _ in 0..CHANNEL_TEST_ITERATIONS {
            let (tx, rx) = super::channel();

            let handles = (0..CHANNEL_TEST_RECEIVERS).map(|_| {
                let rx2 = rx.clone();

                spawn(async move {
                    assert_rx(rx2).await;
                })
            });

            drop(tx);

            timeout(TEST_TIMEOUT, async move {
                for handle in handles {
                    handle.await;
                }
            })
            .await
            .expect("test timeout");
        }
    }
}
