use std::task::Waker;

use atomic::{Atomic, Ordering};

use crate::PollRecv;

use super::oneshot_cell::{OneshotCell, TryRecvError};

#[derive(Copy, Clone)]
enum State {
    Alive,
    Dead,
}

pub struct Transfer<T> {
    sender: Atomic<State>,
    receiver: Atomic<State>,
    value: OneshotCell<T>,
    waker: OneshotCell<Waker>,
}

impl<T> Transfer<T> {
    pub fn new() -> Self {
        Self {
            sender: Atomic::new(State::Alive),
            receiver: Atomic::new(State::Alive),
            value: OneshotCell::new(),
            waker: OneshotCell::new(),
        }
    }

    pub fn send(&self, value: T) -> Result<(), T> {
        if let State::Dead = self.receiver.load(Ordering::Acquire) {
            return Err(value);
        }

        self.value.send(value)?;

        match self.waker.try_recv() {
            Ok(waker) => waker.wake(),
            Err(_) => {}
        };

        Ok(())
    }

    pub fn recv(&self, waker: &Waker) -> PollRecv<T> {
        match self.value.try_recv() {
            Ok(value) => PollRecv::Ready(value),
            Err(TryRecvError::Pending) => {
                if let State::Dead = self.sender.load(Ordering::Acquire) {
                    return PollRecv::Closed;
                }

                self.waker.send(waker.clone()).unwrap();

                PollRecv::Pending
            }
            Err(TryRecvError::Closed) => PollRecv::Closed,
        }
    }

    pub fn sender_disconnect(&self) {
        self.sender.store(State::Dead, Ordering::Release);
    }

    pub fn receiver_disconnect(&self) {
        self.receiver.store(State::Dead, Ordering::Release);
    }
}
