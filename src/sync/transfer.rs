use std::{task::Waker};



use crate::PollRecv;

use super::{
    oneshot_cell::{OneshotCell, TryRecvError},
};

pub struct Transfer<T> {
    state: OneshotCell<T>,
    waker: OneshotCell<Waker>,
}

impl<T> Transfer<T> {
    pub fn new() -> Self {
        Self {
            state: OneshotCell::new(),
            waker: OneshotCell::new(),
        }
    }

    pub fn send(&self, value: T) -> Result<(), T> {
        self.state.send(value)?;

        match self.waker.try_recv() {
            Ok(waker) => waker.wake(),
            Err(_) => {}
        };

        Ok(())
    }

    pub fn recv(&self, waker: &Waker) -> PollRecv<T> {
        match self.state.try_recv() {
            Ok(value) => PollRecv::Ready(value),
            Err(TryRecvError::Pending) => {
                self.waker.send(waker.clone()).unwrap();

                PollRecv::Pending
            }
            Err(TryRecvError::Closed) => PollRecv::Closed,
        }
    }
}
