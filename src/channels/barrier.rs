use std::sync::Arc;

use atomic::{Atomic, Ordering};
use static_assertions::{assert_impl_all, assert_not_impl_all};

use crate::{sync::notifier::Notifier, PollRecv, PollSend, Sink, Stream};

pub fn channel() -> (Sender, Receiver) {
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

pub struct Sender {
    pub(in crate::channels::barrier) shared: Arc<Shared>,
}

assert_impl_all!(Sender: Send);
assert_not_impl_all!(Sender: Clone);

impl Sink for Sender {
    type Item = ();

    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
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
        cx: &mut std::task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        match self.shared.state.load(Ordering::Acquire) {
            State::Pending => {
                self.shared.notify_rx.subscribe(cx.waker().clone());
                PollRecv::Pending
            }
            State::Closed => PollRecv::Ready(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use crate::{PollRecv, PollSend, Sink, Stream};
    use futures_test::task::{new_count_waker, noop_context, panic_context};

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
        let mut w_context = Context::from_waker(&w);

        assert_eq!(
            PollRecv::Pending,
            Pin::new(&mut rx).poll_recv(&mut w_context)
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
}
