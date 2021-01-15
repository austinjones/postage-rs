use std::{cell::UnsafeCell, sync::atomic::AtomicUsize};

use super::{
    notifier::Notifier,
    ref_count::{RefCount, TryDecrement},
};
use atomic::{Atomic, Ordering};
use std::fmt::Debug;
use std::task::Context;

#[derive(Copy, Clone, Debug)]
enum State {
    Empty,
    Writing,
    Reading,
}

// atomic choice to take some action
// if pending,
// verify pending state

pub struct ReadReleaseLock<T> {
    state: Atomic<State>,
    // readers that are waiting for the current value
    readers: RefCount,
    // readers that are waiting for the next written value
    next_readers: RefCount,
    value: UnsafeCell<Option<T>>,
    on_write: Notifier,
    on_release: Notifier,
}

unsafe impl<T> Sync for ReadReleaseLock<T> where T: Clone + Send {}

impl<T> ReadReleaseLock<T> {
    pub fn new() -> Self {
        debug_assert!(Atomic::<State>::is_lock_free());

        Self {
            state: Atomic::new(State::Empty),
            readers: RefCount::new(0),
            next_readers: RefCount::new(0),
            value: UnsafeCell::new(None),
            on_write: Notifier::new(),
            on_release: Notifier::new(),
        }
    }

    pub fn try_write<OnWriting>(
        &self,
        value: T,
        cx: &Context<'_>,
        on_writing: OnWriting,
    ) -> TryWrite<T>
    where
        OnWriting: FnOnce(),
    {
        loop {
            match self.state.load(Ordering::Acquire) {
                State::Empty => {
                    if let Err(_e) = self.state.compare_exchange(
                        State::Empty,
                        State::Writing,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        continue;
                    }

                    on_writing();

                    unsafe {
                        *self.value.get() = Some(value);
                    }

                    loop {
                        let prev = self.next_readers.take();

                        if prev == 0 {
                            break;
                        }

                        self.readers.add(prev);
                    }

                    self.state.store(State::Reading, Ordering::Release);
                    self.on_write.notify();
                    return TryWrite::Ready;
                }
                State::Writing => {
                    self.on_release.subscribe(cx.waker().clone());
                    match self.state.load(Ordering::Acquire) {
                        State::Writing | State::Reading => return TryWrite::Pending(value),
                        _ => continue,
                    }
                }
                State::Reading => {
                    self.on_release.subscribe(cx.waker().clone());
                    match self.state.load(Ordering::Acquire) {
                        State::Reading => return TryWrite::Pending(value),
                        _ => continue,
                    }
                }
            }
        }
    }

    pub fn subscribe_write(&self, context: &Context<'_>) {
        self.on_write.subscribe(context.waker().clone());
    }

    pub fn subscribe_release(&self, context: &Context<'_>) {
        self.on_release.subscribe(context.waker().clone());
    }

    pub fn acquire_next(&self) {
        self.next_readers.increment()
    }

    pub fn decrement_next(&self) -> TryDecrement {
        self.next_readers.decrement()
    }

    pub fn acquire(&self) {
        self.readers.increment()
    }

    pub fn decrement(&self) -> TryDecrement {
        self.readers.decrement()
    }

    pub fn release(&self) {
        assert!(self.readers.load(Ordering::Acquire) == 0);
        self.state.store(State::Empty, Ordering::Release);
        self.on_release.notify();
        // loop {
        //     match self.state.load(Ordering::Acquire) {
        //         State::Empty | State::Writing => {
        //             return self.readers.decrement();
        //         }
        //         State::Reading => {
        //             let decrement = self.readers.decrement();

        //             match decrement {
        //                 TryDecrement::Alive => return decrement,
        //                 TryDecrement::Dead => {}
        //             }

        //             if let Err(e) = self.state.compare_exchange(
        //                 State::Reading,
        //                 State::Empty,
        //                 Ordering::AcqRel,
        //                 Ordering::Relaxed,
        //             ) {
        //                 continue;
        //             }

        //             self.on_release.notify();

        //             return decrement;
        //         }
        //     }
        // }
    }
}

impl<T> ReadReleaseLock<T>
where
    T: Clone,
{
    pub fn try_read(&self, cx: &Context<'_>) -> TryRead<T> {
        loop {
            match self.state.load(Ordering::Acquire) {
                State::Writing => continue,
                State::Empty => {
                    self.on_write.subscribe(cx.waker().clone());

                    match self.state.load(Ordering::Acquire) {
                        State::Empty => return TryRead::Pending,
                        _ => continue,
                    }
                }
                State::Reading => {
                    if self.readers.load(Ordering::Acquire) == 0 {
                        return TryRead::NotLocked;
                    }

                    match self.state.load(Ordering::Acquire) {
                        State::Reading => {
                            let value = unsafe { self.clone_value() };
                            return TryRead::Read(value);
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    unsafe fn clone_value(&self) -> T {
        let reference = self.value.get();
        let r = reference.as_ref().unwrap();
        r.as_ref().unwrap().clone()
    }

    unsafe fn take_value(&self) -> T {
        let reference = self.value.get();
        let r = reference.as_mut().unwrap();
        r.take().unwrap()
    }
}

impl<T> Debug for ReadReleaseLock<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slot")
            .field("state", &self.state)
            .field("readers", &self.readers)
            .finish()
    }
}
pub enum TryRead<T> {
    NotLocked,
    Read(T),
    Pending,
}

pub enum TryClose {
    Released,
    Closed,
}

pub enum TryWrite<T> {
    Ready,
    Pending(T),
}
