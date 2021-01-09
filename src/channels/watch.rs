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
        self.shared.notify_receivers();

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
        if self.shared.extension().generation()
            <= self.generation.load(std::sync::atomic::Ordering::SeqCst)
        {
            if self.shared.is_closed() {
                return PollRecv::Closed;
            }

            self.shared.subscribe_send(cx.waker().clone());
            return PollRecv::Pending;
        }

        self.generation.fetch_add(1, Ordering::AcqRel);
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

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use crate::{PollRecv, PollSend, Sink, Stream};
    use futures_test::task::{new_count_waker, noop_context, panic_context};

    use super::{channel, Receiver, Sender};

    fn pin<'a, 'b>(
        chan: &mut (Sender<State>, Receiver<State>),
    ) -> (Pin<&mut Sender<State>>, Pin<&mut Receiver<State>>) {
        let tx = Pin::new(&mut chan.0);
        let rx = Pin::new(&mut chan.1);

        (tx, rx)
    }

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
}
