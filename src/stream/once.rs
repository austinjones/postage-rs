use std::{cell::UnsafeCell, pin::Pin};

use atomic::{Atomic, Ordering};

use crate::{PollRecv, Stream};

use crate::Context;
#[derive(Copy, Clone)]
enum State {
    Ready,
    Taken,
}

pub struct OnceStream<T> {
    state: Atomic<State>,
    data: UnsafeCell<Option<T>>,
}

impl<T> OnceStream<T> {
    pub fn new(item: T) -> Self {
        Self {
            state: Atomic::new(State::Ready),
            data: UnsafeCell::new(Some(item)),
        }
    }
}

impl<T> Stream for OnceStream<T> {
    type Item = T;

    fn poll_recv(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> crate::PollRecv<Self::Item> {
        if let Ok(_) = self.state.compare_exchange(
            State::Ready,
            State::Taken,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            let value = unsafe {
                let reference = self.data.get().as_mut().unwrap();
                reference.take().unwrap()
            };

            return PollRecv::Ready(value);
        }

        PollRecv::Closed
    }
}
