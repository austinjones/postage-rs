use std::sync::Arc;

use static_assertions::{assert_impl_all, assert_not_impl_all};

use crate::{sync::transfer::Transfer, PollSend, Sink, Stream};

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
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

assert_impl_all!(Sender<String>: Send);
assert_not_impl_all!(Sender<String>: Clone);

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

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.shared.sender_disconnect();
    }
}

pub struct Receiver<T> {
    pub(in crate::channels::oneshot) shared: Arc<Transfer<T>>,
}

assert_impl_all!(Sender<String>: Send);
assert_not_impl_all!(Sender<String>: Clone);

impl<T> Stream for Receiver<T> {
    type Item = T;

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        self.shared.recv(cx.waker())
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.shared.receiver_disconnect();
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use crate::{PollRecv, PollSend, Sink, Stream};
    use futures_test::task::{new_count_waker, noop_context, panic_context};

    use super::channel;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Message(usize);

    #[test]
    fn send_accepted() {
        let mut cx = noop_context();
        let (mut tx, _rx) = channel();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
        assert_eq!(
            PollSend::Rejected(Message(2)),
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
    }

    #[test]
    fn send_recv() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );
        assert_eq!(PollRecv::Closed, Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn sender_disconnect() {
        let mut cx = noop_context();
        let (tx, mut rx) = channel::<Message>();

        drop(tx);

        assert_eq!(PollRecv::Closed, Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn send_then_disconnect() {
        let mut cx = noop_context();
        let (mut tx, mut rx) = channel();

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        drop(tx);

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(PollRecv::Closed, Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn receiver_disconnect() {
        let mut cx = noop_context();
        let (mut tx, rx) = channel();

        drop(rx);

        assert_eq!(
            PollSend::Rejected(Message(1)),
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );
    }

    #[test]
    fn wake_receiver() {
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel();

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
            PollSend::Rejected(Message(2)),
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );

        assert_eq!(1, w1_count.get());
    }
}

#[cfg(test)]
mod tokio_tests {
    use tokio::task::spawn;

    use crate::{Sink, Stream};

    use super::channel;

    #[tokio::test]
    async fn simple() {
        let (mut tx, mut rx) = channel();

        spawn(async move { tx.send(100usize).await });

        assert_eq!(Some(100usize), rx.recv().await);
    }
}

#[cfg(test)]
mod async_std_tests {
    use async_std::task::spawn;

    use crate::{Sink, Stream};

    use super::channel;

    #[async_std::test]
    async fn simple() {
        let (mut tx, mut rx) = channel();

        spawn(async move { tx.send(100usize).await });

        assert_eq!(Some(100usize), rx.recv().await);
    }
}
