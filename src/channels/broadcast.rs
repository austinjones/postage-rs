use std::{cell::UnsafeCell, sync::atomic::AtomicUsize};

use atomic::{Atomic, Ordering};
use static_assertions::assert_impl_all;

use crate::{
    sync::{shared, ReceiverShared, SenderShared},
    PollRecv, PollSend, Sink, Stream,
};

use std::fmt::Debug;

// bounded mpmc with backpressure
pub fn channel<T: Clone + Send>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    // we add one spare capacity so that receivers have an empty slot to wait on
    let (tx_shared, rx_shared) = shared(StateExtension::new(capacity + 1));
    let sender = Sender { shared: tx_shared };

    let receiver = Receiver::new(rx_shared);

    (sender, receiver)
}

pub struct Sender<T> {
    pub(in crate::channels::broadcast) shared: SenderShared<StateExtension<T>>,
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
        mut value: Self::Item,
    ) -> crate::PollSend<Self::Item> {
        if self.shared.is_closed() {
            return PollSend::Rejected(value);
        }

        // start at the head
        // if the next element has references,
        //   register for wakeup
        // else
        //   overwrite the element
        let state = self.shared.extension();

        loop {
            let head = state.head.load(Ordering::Acquire);
            let head_slot = &state.buffer[head % state.buffer.len()];
            let next_slot = &state.buffer[(head + 1) % state.buffer.len()];

            // keep the next slot free, so readers have a location to pause if the buffer fills
            match next_slot.state.load(Ordering::Acquire) {
                SlotState::None => {}
                SlotState::Writing | SlotState::Reading => {
                    self.shared.subscribe_recv(cx.waker().clone());
                    return PollSend::Pending(value);
                }
            }

            // println!("Write slot: {:?}", head_slot);

            match head_slot.write(value, &state.readers) {
                Ok(_) => {
                    return {
                        state
                            .head
                            .compare_and_swap(head, head + 1, Ordering::AcqRel);

                        self.shared.notify_receivers();
                        // println!("Return slot: {:?}", head_slot);
                        PollSend::Ready
                    };
                }
                Err((slot_state, returned_value)) => match slot_state {
                    SlotState::None => unreachable!(),
                    SlotState::Writing => {
                        value = returned_value;
                        continue;
                    }
                    SlotState::Reading => {
                        self.shared.subscribe_recv(cx.waker().clone());
                        return PollSend::Pending(returned_value);
                    }
                },
            }
        }
    }
}

pub struct Receiver<T> {
    shared: ReceiverShared<StateExtension<T>>,
    location: usize,
}

assert_impl_all!(Receiver<String>: Send, Clone);

impl<T> Receiver<T> {
    fn new(shared: ReceiverShared<StateExtension<T>>) -> Self {
        let state = shared.extension();
        state.readers.fetch_add(1, Ordering::AcqRel);

        Self {
            shared,
            location: 0,
        }
    }
}

impl<T> Stream for Receiver<T>
where
    T: Clone,
{
    type Item = T;

    fn poll_recv(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        let state = self.shared.extension();
        let slot = &state.buffer[self.location];
        let next_location = (self.location + 1) % state.buffer.len();

        // println!("Recv buffer: {:?}", &state.buffer);
        // println!("Recv slot: {:?}", &slot);
        loop {
            match slot.state.load(Ordering::Acquire) {
                SlotState::None => {
                    if self.shared.is_closed() {
                        return PollRecv::Closed;
                    }

                    self.shared.subscribe_send(cx.waker().clone());
                    return PollRecv::Pending;
                }
                // TODO: try to avoid spinlock
                SlotState::Writing => {
                    continue;
                }
                SlotState::Reading => {
                    debug_assert!(slot.readers.load(Ordering::Acquire) > 0);

                    let value = unsafe { slot.clone_value() };

                    if let Ok(_) =
                        slot.readers
                            .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Relaxed)
                    {
                        slot.state.store(SlotState::None, Ordering::Release);
                        state.tail.fetch_add(1, Ordering::AcqRel);
                        self.shared.notify_senders();
                    } else {
                        slot.readers.fetch_sub(1, Ordering::AcqRel);
                    }

                    self.location = next_location;

                    return PollRecv::Ready(value);
                }
            }
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        let state = self.shared.extension();

        state.readers.fetch_add(1, Ordering::AcqRel);

        let tail = state.tail.load(Ordering::Acquire);
        let tail_elem = &state.buffer[tail];

        match tail_elem.state.load(Ordering::Acquire) {
            SlotState::None | SlotState::Writing => {}
            SlotState::Reading => {
                tail_elem.readers.fetch_add(1, Ordering::AcqRel);
            }
        }

        Self {
            shared: self.shared.clone(),
            location: tail,
        }
    }
}

struct StateExtension<T> {
    buffer: Box<[Slot<T>]>,
    tail: AtomicUsize,
    head: AtomicUsize,
    readers: AtomicUsize,
}

impl<T> StateExtension<T>
where
    T: Clone + Send,
{
    pub fn new(capacity: usize) -> Self {
        let mut vec = Vec::with_capacity(capacity);

        for _ in 0..capacity {
            vec.push(Slot::new());
        }

        Self {
            buffer: vec.into_boxed_slice(),
            tail: AtomicUsize::new(0),
            head: AtomicUsize::new(0),
            readers: AtomicUsize::new(0),
        }
    }
}

// a ring buffer
// each element has a refcount
// writers block if the next element refcount is non-zero
// readers block if the next element generation is discontinuous
//

#[derive(Copy, Clone, Debug)]
enum SlotState {
    None,
    Writing,
    Reading,
}

#[derive(Debug)]
struct Slot<T> {
    value: UnsafeCell<Option<T>>,
    state: Atomic<SlotState>,
    readers: AtomicUsize,
}

impl<T> Slot<T>
where
    T: Clone,
{
    pub fn new() -> Self {
        Self {
            value: UnsafeCell::new(None),
            state: Atomic::new(SlotState::None),
            readers: AtomicUsize::new(0),
        }
    }

    pub unsafe fn clone_value(&self) -> T {
        let reference = self.value.get();
        let r = reference.as_ref().unwrap();
        r.as_ref().unwrap().clone()
    }

    pub fn write(&self, value: T, readers: &AtomicUsize) -> Result<(), (SlotState, T)> {
        if let Err(e) = self.state.compare_exchange(
            SlotState::None,
            SlotState::Writing,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            return Err((e, value));
        }

        unsafe {
            *self.value.get() = Some(value);
        }

        let readers = readers.load(Ordering::Acquire);
        self.readers.store(readers, Ordering::Release);

        self.state.store(SlotState::Reading, Ordering::Release);
        Ok(())
    }
}

unsafe impl<T> Sync for Slot<T> where T: Clone + Send {}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Context};

    use crate::{PollRecv, PollSend, Sink, Stream};
    use futures_test::task::{new_count_waker, noop_context, panic_context};

    use super::{channel, Receiver, Sender};

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
        let mut cx = panic_context();
        let mut chan = channel(1);
        let (tx, _rx) = pin(&mut chan);

        assert_eq!(PollSend::Ready, tx.poll_send(&mut cx, Message(1)));
    }

    #[test]
    fn full_send_blocks() {
        let mut cx = panic_context();
        let (mut tx, _rx) = channel(1);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        assert_eq!(
            PollSend::Pending(Message(2)),
            Pin::new(&mut tx).poll_send(&mut cx, Message(2))
        );
    }

    #[test]
    fn empty_blocks() {
        let mut cx = noop_context();
        let mut chan = channel(1);
        let (_tx, rx) = pin(&mut chan);

        assert_eq!(PollRecv::Pending, rx.poll_recv(&mut cx));
    }

    #[test]
    fn send_recv() {
        let mut cx = noop_context();
        let mut chan = channel(1);
        let (tx, rx) = pin(&mut chan);

        assert_eq!(PollSend::Ready, tx.poll_send(&mut cx, Message(1)));
        assert_eq!(PollRecv::Ready(Message(1)), rx.poll_recv(&mut cx));
    }

    #[test]
    fn two_senders_recv() {
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

        assert_eq!(PollRecv::Pending, Pin::new(&mut rx).poll_recv(&mut cx));
    }

    #[test]
    fn two_receivers() {
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
        let mut cx = panic_context();
        let (mut tx, mut rx) = channel(1);

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, Message(1))
        );

        let (w2, w2_count) = new_count_waker();
        let mut w2_context = Context::from_waker(&w2);
        assert_eq!(
            PollSend::Pending(Message(2)),
            Pin::new(&mut tx).poll_send(&mut w2_context, Message(2))
        );

        assert_eq!(0, w2_count.get());

        assert_eq!(
            PollRecv::Ready(Message(1)),
            Pin::new(&mut rx).poll_recv(&mut cx)
        );

        assert_eq!(1, w2_count.get());
        assert_eq!(PollRecv::Pending, Pin::new(&mut rx).poll_recv(&mut cx));

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
}
