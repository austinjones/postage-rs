//! A fixed-capacity multi-producer, single-consumer channel.  
//! The producer can be cloned, and the sender task is suspended if the channel becomes full.

use super::SendMessage;
use crate::{
    sink::{PollSend, Sink},
    stream::{PollRecv, Stream},
    sync::{shared, ReceiverShared, SenderShared},
};
use crossbeam_queue::ArrayQueue;
use static_assertions::{assert_impl_all, assert_not_impl_all};

pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    #[cfg(feature = "debug")]
    log::error!("Creating mpsc channel with capacity {}", capacity);
    let (tx_shared, rx_shared) = shared(StateExtension::new(capacity));
    let sender = Sender { shared: tx_shared };

    let receiver = Receiver { shared: rx_shared };

    (sender, receiver)
}

/// The sender half of an mpsc channel.  Can send messages with the postage::Sink trait.
///
/// Can be cloned.
pub struct Sender<T> {
    pub(in crate::channels::mpsc) shared: SenderShared<StateExtension<T>>,
}

assert_impl_all!(Sender<String>: Clone, Send, Sync);

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<T> Sink for Sender<T> {
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        cx: &mut crate::Context<'_>,
        value: Self::Item,
    ) -> PollSend<Self::Item> {
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
                self.shared.subscribe_recv(cx);

                if self.shared.is_closed() {
                    return PollSend::Rejected(v);
                }

                match queue.push(v) {
                    Ok(_) => {
                        self.shared.notify_receivers();
                        PollSend::Ready
                    }
                    Err(v) => PollSend::Pending(v),
                }
            }
        }
    }
}

/// The receiver half of an mpsc channel.  Cannot be cloned.
///
/// Can receive messages with the postage::Stream trait.
pub struct Receiver<T> {
    pub(in crate::channels::mpsc) shared: ReceiverShared<StateExtension<T>>,
}

assert_impl_all!(Receiver<SendMessage>: Send, Sync);
assert_not_impl_all!(Receiver<SendMessage>: Clone);

impl<T> Stream for Receiver<T> {
    type Item = T;

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut crate::Context<'_>,
    ) -> PollRecv<Self::Item> {
        match self.shared.extension().queue.pop() {
            Some(v) => {
                self.shared.notify_senders();
                PollRecv::Ready(v)
            }
            None => {
                if self.shared.is_closed() {
                    return PollRecv::Closed;
                }

                self.shared.subscribe_send(cx);

                match self.shared.extension().queue.pop() {
                    Some(v) => {
                        self.shared.notify_senders();
                        PollRecv::Ready(v)
                    }
                    None => {
                        if self.shared.is_closed() {
                            return PollRecv::Closed;
                        }
                        PollRecv::Pending
                    }
                }
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

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use crate::{
        sink::{PollSend, Sink},
        stream::{PollRecv, Stream},
        test::{noop_context, panic_context},
    };
    use futures_test::task::new_count_waker;

    use super::{channel, Receiver, Sender};

    fn pin<'a, 'b>(
        chan: &mut (Sender<Message>, Receiver<Message>),
    ) -> (Pin<&mut Sender<Message>>, Pin<&mut Receiver<Message>>) {
        let tx = Pin::new(&mut chan.0);
        let rx = Pin::new(&mut chan.1);

        (tx, rx)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Message(usize);

    #[test]
    fn send_accepted() {
        let mut cx = panic_context();
        let mut chan = channel(2);
        let (tx, _) = pin(&mut chan);

        assert_eq!(PollSend::Ready, tx.poll_send(&mut cx, Message(1)));
    }

    #[test]
    fn send_blocks() {
        let mut cx = panic_context();
        let (mut tx, _rx) = channel(2);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
    }

    #[test]
    fn send_recv() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(2);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
        assert_eq!(
            PollSend::Pending(Message(3)),
            Pin::new(&mut tx).poll_send(&mut noop_context(), Message(3))
        );

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(
            PollRecv::Ready(Message(2)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut noop_context())
        );
    }

    #[test]
    fn sender_disconnect() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(100);
        let mut tx2 = tx.clone();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx2).poll_send(&mut cx, Message(2))
        );

        drop(tx);
        drop(tx2);

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(
            PollRecv::Ready(Message(2)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(PollRecv::Closed, Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn receiver_disconnect() {
        let mut cx = panic_context();
        let (mut tx, rx) = channel(100);
        let mut tx2 = tx.clone();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx2).poll_send(&mut cx, Message(2))
        );

        drop(rx);

        assert_eq!(
            PollSend::Rejected(Message(3)),
            Pin::new(&mut tx).poll_send(&mut cx, Message(3))
        );

        assert_eq!(
            PollSend::Rejected(Message(4)),
            Pin::new(&mut tx2).poll_send(&mut cx, Message(4))
        );
    }

    #[test]
    fn wake_sender() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(1);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        let (w2, w2_count) = new_count_waker();
        let w2_context = Context::from_waker(&w2);
        assert_eq!(
            PollSend::Pending(Message(2)),
            Pin::new(&mut tx).poll_send(&mut w2_context.into(), Message(2))
        );

        assert_eq!(0, w2_count.get());

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(1, w2_count.get());
        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut noop_context())
        );

        assert_eq!(1, w2_count.get());
    }

    #[test]
    fn wake_receiver() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(100);

        let (w1, w1_count) = new_count_waker();
        let w1_context = Context::from_waker(&w1);

        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w1_context.into())
        );

        assert_eq!(0, w1_count.get());

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        assert_eq!(1, w1_count.get());

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );

        assert_eq!(1, w1_count.get());
    }

    #[test]
    fn wake_sender_on_disconnect() {
        let (mut tx, rx) = channel(1);

        let (w1, w1_count) = new_count_waker();
        let w1_context = Context::from_waker(&w1);
        let mut w1_context: crate::Context<'_> = w1_context.into();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut w1_context, Message(1))
        );

        assert_eq!(
            PollSend::Pending(Message(2)),
            Pin::new(&mut tx).poll_send(&mut w1_context, Message(2))
        );

        assert_eq!(0, w1_count.get());

        drop(rx);

        assert_eq!(1, w1_count.get());
    }

    #[test]
    fn wake_receiver_on_disconnect() {
        let (tx, mut rx) = channel::<()>(100);

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
        sink::Sink,
        stream::Stream,
        test::{capacity_iter, Channel, Channels, Message, CHANNEL_TEST_SENDERS, TEST_TIMEOUT},
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn simple() {
        // crate::logging::enable_log();

        for cap in capacity_iter() {
            let (mut tx, mut rx) = super::channel(cap);

            let join = spawn(async move {
                for message in Message::new_iter(0) {
                    tx.send(message).await.expect("send failed");
                }
            });

            let rx_handle = spawn(async move {
                let mut channel = Channel::new(0);
                while let Some(message) = rx.recv().await {
                    channel.assert_message(&message);
                }
                join.await.expect("Join failed");
            });

            timeout(TEST_TIMEOUT, rx_handle)
                .await
                .expect("test timeout")
                .expect("join error");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn multi_sender() {
        for cap in capacity_iter() {
            let (tx, mut rx) = super::channel(cap);

            for i in 0..CHANNEL_TEST_SENDERS {
                let mut tx2 = tx.clone();
                spawn(async move {
                    for message in Message::new_multi_sender(i) {
                        tx2.send(message).await.expect("send failed");
                    }
                });
            }

            drop(tx);

            let rx_handle = spawn(async move {
                let mut channel = Channels::new(CHANNEL_TEST_SENDERS);
                while let Some(message) = rx.recv().await {
                    channel.assert_message(&message);
                }
            });

            timeout(TEST_TIMEOUT, rx_handle)
                .await
                .expect("test timeout")
                .expect("join error");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn clone_monster() {
        for cap in capacity_iter() {
            // SimpleLogger::new()
            //     .with_level(LevelFilter::Warn)
            //     .init()
            //     .unwrap();

            let (tx, mut rx) = super::channel(cap);
            let (mut barrier, mut sender_quit) = crate::barrier::channel();

            let mut tx2 = tx.clone();
            spawn(async move {
                for message in Message::new_iter(0) {
                    tx2.send(message).await.expect("send failed");
                }

                barrier.send(()).await.expect("clone task shutdown failed");
            });

            spawn(async move {
                loop {
                    if let Ok(_) = sender_quit.try_recv() {
                        break;
                    }

                    let tx2 = tx.clone();
                    tokio::time::sleep(Duration::from_micros(100)).await;
                    drop(tx2);

                    tokio::time::sleep(Duration::from_micros(50)).await;
                }
            });

            let rx_handle = spawn(async move {
                let mut channel = Channel::new(0);

                while let Some(message) = rx.recv().await {
                    channel.assert_message(&message);
                }
            });

            timeout(TEST_TIMEOUT, rx_handle)
                .await
                .expect("test timeout")
                .expect("join failed");
        }
    }
}

#[cfg(test)]
mod async_std_tests {
    use std::time::Duration;

    use async_std::{
        future::timeout,
        task::{self, spawn},
    };

    use crate::{
        sink::Sink,
        stream::Stream,
        test::{capacity_iter, Channel, Channels, Message, CHANNEL_TEST_SENDERS, TEST_TIMEOUT},
    };

    #[async_std::test]
    async fn simple() {
        for cap in capacity_iter() {
            let (mut tx, mut rx) = super::channel(cap);

            spawn(async move {
                for message in Message::new_iter(0) {
                    tx.send(message).await.expect("send failed");
                }
            });

            let rx_handle = spawn(async move {
                let mut channel = Channel::new(0);
                while let Some(message) = rx.recv().await {
                    channel.assert_message(&message);
                }
            });

            timeout(TEST_TIMEOUT, rx_handle)
                .await
                .expect("test timeout");
        }
    }

    #[async_std::test]
    async fn multi_sender() {
        for cap in capacity_iter() {
            let (tx, mut rx) = super::channel(cap);

            for i in 0..CHANNEL_TEST_SENDERS {
                let mut tx2 = tx.clone();
                spawn(async move {
                    for message in Message::new_multi_sender(i) {
                        tx2.send(message).await.expect("send failed");
                    }
                });
            }

            drop(tx);

            let rx_handle = spawn(async move {
                let mut channel = Channels::new(CHANNEL_TEST_SENDERS);
                while let Some(message) = rx.recv().await {
                    channel.assert_message(&message);
                }
            });

            timeout(TEST_TIMEOUT, rx_handle)
                .await
                .expect("test timeout");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn clone_monster() {
        // crate::logging::enable_log();

        for cap in capacity_iter() {
            let (tx, mut rx) = super::channel(cap);
            let (mut barrier, mut sender_quit) = crate::barrier::channel();

            let mut tx2 = tx.clone();
            spawn(async move {
                for message in Message::new_iter(0) {
                    tx2.send(message).await.expect("send failed");
                }

                barrier.send(()).await.expect("clone task shutdown failed");
            });

            spawn(async move {
                loop {
                    if let Ok(_) = sender_quit.try_recv() {
                        break;
                    }

                    let tx2 = tx.clone();
                    task::sleep(Duration::from_micros(100)).await;
                    drop(tx2);
                    task::sleep(Duration::from_micros(50)).await;
                }
            });

            let rx_handle = spawn(async move {
                let mut channel = Channel::new(0);

                while let Some(message) = rx.recv().await {
                    channel.assert_message(&message);
                }
            });

            timeout(TEST_TIMEOUT, rx_handle)
                .await
                .expect("test timeout");
        }
    }
}
