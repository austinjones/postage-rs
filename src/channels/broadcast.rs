use static_assertions::assert_impl_all;

use crate::{
    sync::{
        mpmc_circular_buffer::{BufferReader, MpmcCircularBuffer, TryRead, TryWrite},
        shared, ReceiverShared, SenderShared,
    },
    PollRecv, PollSend, Sink, Stream,
};

// bounded mpmc with backpressure
pub fn channel<T: Clone + Send>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    // we add one spare capacity so that receivers have an empty slot to wait on
    let (buffer, reader) = MpmcCircularBuffer::new(capacity);

    let (tx_shared, rx_shared) = shared(buffer);
    let sender = Sender { shared: tx_shared };

    let receiver = Receiver::new(rx_shared, reader);

    (sender, receiver)
}

pub struct Sender<T> {
    pub(in crate::channels::broadcast) shared: SenderShared<MpmcCircularBuffer<T>>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

assert_impl_all!(Sender<String>: Send, Clone);

impl<T> Sink for Sender<T>
where
    T: Clone,
{
    type Item = T;

    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        value: Self::Item,
    ) -> crate::PollSend<Self::Item> {
        if self.shared.is_closed() {
            return PollSend::Rejected(value);
        }

        // start at the head
        // if the next element has references,
        //   register for wakeup
        // else
        //   overwrite the element
        let buffer = self.shared.extension();
        match buffer.try_write(value, cx) {
            TryWrite::Pending(value) => PollSend::Pending(value),
            TryWrite::Ready => PollSend::Ready,
        }
    }
}

impl<T> Sender<T> {
    pub fn subscribe(&self) -> Receiver<T> {
        let shared = self.shared.clone_receiver();
        let reader = shared.extension().new_reader();

        Receiver { shared, reader }
    }
}

pub struct Receiver<T> {
    shared: ReceiverShared<MpmcCircularBuffer<T>>,
    reader: BufferReader,
}

assert_impl_all!(Receiver<String>: Send, Clone);

impl<T> Receiver<T> {
    fn new(shared: ReceiverShared<MpmcCircularBuffer<T>>, reader: BufferReader) -> Self {
        Self { shared, reader }
    }
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
        let buffer = self.shared.extension();
        let reader = &self.reader;

        match reader.try_read(buffer, cx) {
            TryRead::Pending => {
                if self.shared.is_closed() {
                    return PollRecv::Closed;
                }

                self.shared.subscribe_send(cx.waker().clone());
                PollRecv::Pending
            }
            TryRead::Ready(value) => PollRecv::Ready(value),
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        let buffer = self.shared.extension();
        let reader = self.reader.clone(buffer);

        Self {
            shared: self.shared.clone(),
            reader,
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let buffer = self.shared.extension();
        self.reader.drop(buffer);
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use crate::{PollRecv, PollSend, Sink, Stream};
    use futures_test::task::{new_count_waker, noop_context, panic_context};
    

    use super::{channel, Receiver, Sender};

    //TODO: add test covering rx location when cloned on an in-progress channel (exercising tail)
    fn pin<'a, 'b>(
        chan: &mut (Sender<Message>, Receiver<Message>),
    ) -> (Pin<&mut Sender<Message>>, Pin<&mut Receiver<Message>>) {
        let tx = Pin::new(&mut chan.0);
        let rx = Pin::new(&mut chan.1);

        (tx, rx)
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Message(usize);

    #[test]
    fn send_accepted() {
        // SimpleLogger::new().init().unwrap();
        let mut cx = panic_context();
        let mut chan = channel(2);
        let (tx, _rx) = pin(&mut chan);

        assert_eq!(PollSend::Ready, tx.poll_send(&mut cx, Message(1)));
    }

    #[test]
    fn full_send_blocks() {
        // SimpleLogger::new().init().unwrap();
        let mut cx = panic_context();
        let (mut tx, _rx) = channel(2);

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
    }

    #[test]
    fn empty_send_recv() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel(0);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );
    }

    #[test]
    fn send_recv() {
        let mut cx = noop_context();
        let mut chan = channel(2);
        let (tx, rx) = pin(&mut chan);

        assert_eq!(PollSend::Ready, tx.poll_send(&mut cx, Message(1)));
        assert_eq!(PollRecv::Ready(Message(1)), rx.poll_recv(&mut cx));
    }

    #[test]
    fn sender_subscribe_same_read() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel(2);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        let mut rx2 = tx.subscribe();
        assert_eq!(PollRecv::Pending, Pin::new(&mut rx2).poll_recv(&mut cx));
        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
        assert_eq!(
            PollRecv::Ready(Message(2)),
            Pin::new(&mut rx2).poll_recv(&mut cx)
        );
    }

    #[test]
    fn sender_subscribe_different_read() {
        // SimpleLogger::new().init().unwrap();

        let mut cx = noop_context();
        let (mut tx, _rx) = channel(2);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        let mut rx2 = tx.subscribe();
        assert_eq!(PollRecv::Pending, Pin::new(&mut rx2).poll_recv(&mut cx));
        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
        assert_eq!(
            PollRecv::Ready(Message(2)),
            Pin::new(&mut rx2).poll_recv(&mut cx)
        );
    }

    #[test]
    fn two_senders_recv() {
        // SimpleLogger::new().init().unwrap();

        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(2);
        let mut tx2 = tx.clone();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx2).poll_send(&mut cx, Message(2))
        );

        assert_eq!(
            PollSend::Pending(Message(3)),
            Pin::new(&mut tx).poll_send(&mut noop_context(), Message(3))
        );
        assert_eq!(
            PollSend::Pending(Message(3)),
            Pin::new(&mut tx2).poll_send(&mut noop_context(), Message(3))
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
    fn two_receivers() {
        // SimpleLogger::new().init().unwrap();

        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(2);
        let mut rx2 = rx.clone();

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

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx2).poll_recv(&mut cx)
        );
        assert_eq!(
            PollRecv::Ready(Message(2)),
            Pin::new(&mut rx2).poll_recv(&mut cx)
        );
        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx2).poll_recv(&mut noop_context())
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
        let rx2 = rx.clone();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
        drop(rx);
        drop(rx2);
        assert_eq!(
            PollSend::Rejected(Message(2)),
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
    }

    #[test]
    fn wake_sender() {
        // SimpleLogger::new().init().unwrap();

        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(2);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        let (w2, w2_count) = new_count_waker();
        let mut w2_context = Context::from_waker(&w2);
        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
        assert_eq!(
            PollSend::Pending(Message(3)),
            Pin::new(&mut tx).poll_send(&mut w2_context, Message(3))
        );

        assert_eq!(0, w2_count.get());

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(
            PollRecv::Ready(Message(2)),
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
        let mut w1_context = Context::from_waker(&w1);

        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w1_context)
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
    fn dropping_receiver_does_not_block() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(2);
        let rx2 = rx.clone();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );

        drop(rx2);
        assert_eq!(
            PollSend::Pending(Message(2)),
            Pin::new(&mut tx).poll_send(&mut noop_context(), Message(2))
        );

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(4))
        );
    }

    #[test]
    fn drop_receiver_frees_slot() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(2);
        let rx2 = rx.clone();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        drop(rx2);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(3))
        );
    }

    #[test]
    fn wake_sender_on_disconnect() {
        let (mut tx, rx) = channel(2);

        let (w1, w1_count) = new_count_waker();
        let mut w1_context = Context::from_waker(&w1);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut w1_context, Message(1))
        );

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut w1_context, Message(2))
        );

        assert_eq!(
            PollSend::Pending(Message(3)),
            Pin::new(&mut tx).poll_send(&mut w1_context, Message(3))
        );

        assert_eq!(0, w1_count.get());

        drop(rx);

        assert_eq!(1, w1_count.get());
    }

    #[test]
    fn wake_receiver_on_disconnect() {
        let (tx, mut rx) = channel::<()>(100);

        let (w1, w1_count) = new_count_waker();
        let mut w1_context = Context::from_waker(&w1);

        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w1_context)
        );

        assert_eq!(0, w1_count.get());

        drop(tx);

        assert_eq!(1, w1_count.get());
    }

    #[test]
    fn reader_bounds_bug() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel(2);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
        assert_eq!(
            PollRecv::Ready(Message(2)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );
    }

    #[test]
    fn drop_subscribe_bug() {
        // SimpleLogger::new().init().unwrap();

        let mut cx = noop_context();
        let (mut tx, rx) = channel(2);

        drop(rx);
        let mut rx2 = tx.subscribe();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx2).poll_recv(&mut cx)
        );
    }

    #[test]
    fn skips_intermediate_bug() {
        let mut cx = noop_context();
        let (mut tx, rx) = channel(2);

        drop(rx);
        let mut rx2 = tx.subscribe();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx2).poll_recv(&mut cx)
        );
    }

    #[test]
    fn drop_subscribe_ignores_queued() {
        let mut cx = noop_context();
        let (mut tx, rx) = channel(2);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        drop(rx);
        let mut rx2 = tx.subscribe();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
        assert_eq!(
            PollRecv::Ready(Message(2)),
            Pin::new(&mut rx2).poll_recv(&mut cx)
        );
    }

    // #[test]
    // fn sender_receiver_hang_bug() {
    //     let mut cx = noop_context();
    //     let (mut tx, mut rx) = channel(2);

    //     assert_eq!(
    //         PollSend::Ready,
    //         Pin::new(&mut tx).poll_send(&mut cx, Message(1))
    //     );

    //     assert_eq!(
    //         PollRecv::Ready(Message(1)),
    //         Pin::new(&mut rx).poll_recv(&mut cx)
    //     );

    //     assert_eq!(
    //         PollSend::Ready,
    //         Pin::new(&mut tx).poll_send(&mut cx, Message(2))
    //     );

    //     let (w3, w3_count) = new_count_waker();
    //     let mut w3_context = Context::from_waker(&w3);
    //     assert_eq!(
    //         PollSend::Pending(Message(3)),
    //         Pin::new(&mut tx).poll_send(&mut w3_context, Message(3))
    //     );

    //     assert_eq!(
    //         PollRecv::Ready(Message(2)),
    //         Pin::new(&mut rx).poll_recv(&mut cx)
    //     );

    //     assert_eq!(1, w3_count.get());
    // }
}

#[cfg(test)]
mod tokio_tests {
    use tokio::task::{spawn, JoinHandle};

    use crate::{
        test::{Channel, Channels, Message, CHANNEL_TEST_RECEIVERS, CHANNEL_TEST_SENDERS},
        Sink, Stream,
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn simple() {
        // SimpleLogger::new()
        //     .with_level(LevelFilter::Warn)
        //     .init()
        //     .unwrap();
        let (mut tx, mut rx) = super::channel(2);

        spawn(async move {
            for message in Message::new_iter(0) {
                tx.send(message).await.expect("send failed");
            }
        });

        let mut channel = Channel::new(0);
        while let Some(message) = rx.recv().await {
            channel.assert_message(&message);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn multi_sender() {
        // SimpleLogger::new().init().unwrap();

        let (tx, mut rx) = super::channel(2);

        for i in 0..CHANNEL_TEST_SENDERS {
            let mut tx2 = tx.clone();
            spawn(async move {
                for message in Message::new_iter(i) {
                    tx2.send(message).await.expect("send failed");
                    // sleep(Duration::from_millis(5 + 5 * i as u64)).await;
                }
            });
        }

        drop(tx);

        let mut channels = Channels::new(CHANNEL_TEST_SENDERS);
        while let Some(message) = rx.recv().await {
            channels.assert_message(&message);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn multi_receiver() {
        // SimpleLogger::new().init().unwrap();

        let (mut tx, rx) = super::channel(2);

        spawn(async move {
            for message in Message::new_iter(0) {
                tx.send(message).await.expect("send failed");
            }
        });

        let handles: Vec<JoinHandle<()>> = (0..CHANNEL_TEST_RECEIVERS)
            .map(|_| {
                let mut rx2 = rx.clone();
                let mut channels = Channels::new(1);

                spawn(async move {
                    while let Some(message) = rx2.recv().await {
                        channels.assert_message(&message);
                    }
                })
            })
            .collect();

        drop(rx);

        for handle in handles {
            handle.await.expect("Assertion failure");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn multi_sender_multi_receiver() {
        let (tx, rx) = super::channel(4);

        for i in 0..CHANNEL_TEST_SENDERS {
            let mut tx2 = tx.clone();
            spawn(async move {
                for message in Message::new_iter(i) {
                    tx2.send(message).await.expect("send failed");
                }
            });
        }

        drop(tx);

        let handles: Vec<JoinHandle<()>> = (0..CHANNEL_TEST_RECEIVERS)
            .map(|_i| {
                let mut rx2 = rx.clone();
                let mut channels = Channels::new(CHANNEL_TEST_SENDERS);

                spawn(async move {
                    while let Some(message) = rx2.recv().await {
                        channels.assert_message(&message);
                    }
                })
            })
            .collect();

        drop(rx);

        for handle in handles {
            handle.await.expect("Assertion failure");
        }
    }
}

#[cfg(test)]
mod async_std_tests {
    use async_std::task::{spawn, JoinHandle};

    use crate::{
        test::{Channel, Channels, Message, CHANNEL_TEST_RECEIVERS, CHANNEL_TEST_SENDERS},
        Sink, Stream,
    };

    #[async_std::test]
    async fn simple() {
        let (mut tx, mut rx) = super::channel(4);

        spawn(async move {
            for message in Message::new_iter(0) {
                tx.send(message).await.expect("send failed");
            }
        });

        let mut channel = Channel::new(0);
        while let Some(message) = rx.recv().await {
            channel.assert_message(&message);
        }
    }

    #[async_std::test]
    async fn multi_sender() {
        let (tx, mut rx) = super::channel(4);

        for i in 0..CHANNEL_TEST_SENDERS {
            let mut tx2 = tx.clone();
            spawn(async move {
                for message in Message::new_iter(i) {
                    tx2.send(message).await.expect("send failed");
                }
            });
        }

        drop(tx);

        let mut channels = Channels::new(CHANNEL_TEST_SENDERS);
        while let Some(message) = rx.recv().await {
            channels.assert_message(&message);
        }
    }

    #[async_std::test]
    async fn multi_receiver() {
        let (mut tx, rx) = super::channel(4);

        spawn(async move {
            for message in Message::new_iter(0) {
                tx.send(message).await.expect("send failed");
            }
        });

        let handles: Vec<JoinHandle<()>> = (0..CHANNEL_TEST_RECEIVERS)
            .map(|_| {
                let mut rx2 = rx.clone();
                let mut channels = Channels::new(1);

                spawn(async move {
                    while let Some(message) = rx2.recv().await {
                        channels.assert_message(&message);
                    }
                })
            })
            .collect();

        drop(rx);

        for handle in handles {
            handle.await;
        }
    }

    #[async_std::test]
    async fn multi_sender_multi_receiver() {
        let (tx, rx) = super::channel(4);

        for i in 0..CHANNEL_TEST_SENDERS {
            let mut tx2 = tx.clone();
            spawn(async move {
                for message in Message::new_iter(i) {
                    tx2.send(message).await.expect("send failed");
                }
            });
        }

        drop(tx);

        let handles: Vec<JoinHandle<()>> = (0..CHANNEL_TEST_RECEIVERS)
            .map(|_i| {
                let mut rx2 = rx.clone();
                let mut channels = Channels::new(CHANNEL_TEST_SENDERS);

                spawn(async move {
                    while let Some(message) = rx2.recv().await {
                        channels.assert_message(&message);
                    }
                })
            })
            .collect();

        drop(rx);

        for handle in handles {
            handle.await;
        }
    }
}
