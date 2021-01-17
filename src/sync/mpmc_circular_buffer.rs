use std::{
    cmp::max,
    sync::{atomic::AtomicUsize, Mutex},
};

use atomic::Ordering;
use std::task::Context;

use super::{
    ref_count::TryDecrement,
    rr_lock::{self, ReadReleaseLock},
};
use std::fmt::Debug;

// A lock-free multi-producer, multi-consumer circular buffer
// Each reader will see each value created exactly once.
// Cloned readers inherit the read location of the reader that was cloned.

pub struct MpmcCircularBuffer<T> {
    buffer: Box<[ReadReleaseLock<T>]>,
    head_lock: Mutex<()>,
    head: AtomicUsize,
    tail_lock: Mutex<()>,
    tail: AtomicUsize,
}

impl<T> Debug for MpmcCircularBuffer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MpmcCircularBuffer")
            .field("buffer", &self.buffer)
            .field("head", &self.head)
            .field("tail", &self.tail)
            .finish()
    }
}

impl<T> MpmcCircularBuffer<T>
where
    T: Clone,
{
    pub fn new(capacity: usize) -> (Self, BufferReader) {
        // we require two readers, so that unique slots can be acquired and released
        let capacity = max(2, capacity);
        let mut vec = Vec::with_capacity(capacity);

        for _ in 0..capacity {
            vec.push(ReadReleaseLock::new());
        }

        let this = Self {
            buffer: vec.into_boxed_slice(),
            head: AtomicUsize::new(0),
            head_lock: Mutex::new(()),
            tail: AtomicUsize::new(0),
            tail_lock: Mutex::new(()),
        };

        let reader = BufferReader {
            index: 0,
            state: ReaderState::Blocked,
        };

        this.buffer[0].acquire();

        (this, reader)
    }
}

pub enum TryWrite<T> {
    Pending(T),
    Ready,
}

impl<T> MpmcCircularBuffer<T> {
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn try_write(&self, value: T, cx: &Context<'_>) -> TryWrite<T> {
        let head_lock = self.head_lock.lock().unwrap();
        let head_id = self.head.load(Ordering::SeqCst);
        let next_id = head_id + 1;
        let head_slot = self.get_slot(head_id);

        match head_slot.try_write(value, cx, move || {
            self.head.store(next_id, Ordering::SeqCst);
            drop(head_lock);
        }) {
            rr_lock::TryWrite::Ready => {
                #[cfg(feature = "debug")]
                {
                    // the head is writing into the tail slot
                    // the reader thread is *at* the tail slot
                    let tail = self.tail.load(Ordering::Acquire);
                    let tail_distance = head_id - tail;
                    if tail_distance >= self.buffer.len() {
                        let head_slot = head_id % self.len();
                        let tail_slot = head_id % self.len();
                        log::error!(
                            "[{}] Write clobbered current tail {} (slot {} / {})",
                            head_id,
                            tail,
                            head_slot,
                            tail_slot
                        );
                    }
                }

                let slot_index = head_id % self.len();
                #[cfg(feature = "debug")]
                log::warn!(
                    "[{}] Write complete in slot {}, head incremented from {} to {}",
                    head_id,
                    slot_index,
                    head_id,
                    next_id
                );

                return TryWrite::Ready;
            }
            rr_lock::TryWrite::Pending(v) => {
                #[cfg(feature = "debug")]
                log::debug!(
                    "[{}] Write pending, slot is reading: {:?}",
                    head_id,
                    head_slot
                );
                return TryWrite::Pending(v);
            }
        }
    }

    pub fn new_reader(&self) -> BufferReader {
        let head_lock = self.head_lock.lock().unwrap();
        let index = self.head.load(Ordering::SeqCst);
        let slot = self.get_slot(index);

        slot.acquire_next();
        drop(head_lock);

        #[cfg(feature = "debug")]
        log::info!("[{}] New reader", index);

        BufferReader {
            index,
            state: ReaderState::Blocked,
        }
    }

    pub(in crate::sync::mpmc_circular_buffer) fn get_slot(&self, id: usize) -> &ReadReleaseLock<T> {
        let index = id % self.len();
        &self.buffer[index]
    }

    fn release_tail(&self) {
        let _tail_lock = self.head_lock.lock().unwrap();
        loop {
            // let _head_lock = self.head_lock.lock().unwrap();

            let tail = self.tail.load(Ordering::Acquire);
            let slot = self.get_slot(tail);
            if let TryDecrement::Alive(_) = slot.release() {
                return;
            }

            self.tail.store(tail + 1, Ordering::Release);
            #[cfg(feature = "debug")]
            log::warn!(
                "[{}] Released slot, tail incremented from {} to {}",
                tail,
                tail,
                tail + 1
            );
        }
    }
}

#[derive(Debug)]
pub struct BufferReader {
    index: usize,
    state: ReaderState,
}

#[derive(Copy, Clone, Debug)]
enum ReaderState {
    Reading,
    Blocked,
}

pub enum TryRead<T> {
    /// A value is ready
    Ready(T),
    /// A value is pending in this slot
    Pending,
}

impl BufferReader {
    pub fn try_read<T>(&mut self, buffer: &MpmcCircularBuffer<T>, cx: &Context<'_>) -> TryRead<T>
    where
        T: Clone,
    {
        let index = self.index;

        match self.state {
            ReaderState::Blocked => {
                let head_lock = buffer.head_lock.lock().unwrap();
                let head = buffer.head.load(Ordering::Acquire);

                if index >= head {
                    let slot = buffer.get_slot(index);
                    slot.subscribe_write(cx);

                    #[cfg(feature = "debug")]
                    log::debug!(
                        "[{}] TryRead still blocked on head {}, slot: {:?}",
                        index,
                        head,
                        slot
                    );

                    return TryRead::Pending;
                }

                #[cfg(feature = "debug")]
                log::debug!("[{}] TryRead un-blocked", index);

                self.state = ReaderState::Reading;
            }
            ReaderState::Reading => {}
        }

        let slot = buffer.get_slot(index);

        // #[cfg(feature = "debug")]
        // trace!("TryRead {}, slot: {:?}", index, slot);

        match slot.try_read(cx) {
            rr_lock::TryRead::NotLocked => {
                panic!("[{}] MPMC slot not locked! {:?}", index, slot);
            }
            rr_lock::TryRead::Read(value) => {
                let head = buffer.head.load(Ordering::Acquire);
                if index >= head {
                    #[cfg(feature = "debug")]
                    log::error!(
                        "[{}] Value was read, but head {} advanced during read.",
                        index,
                        head
                    );

                    return TryRead::Pending;
                }
                self.advance(index, buffer, cx);
                self.release(index, buffer);

                #[cfg(feature = "debug")]
                log::debug!("[{}] Read complete", index);

                return TryRead::Ready(value);
            }
            rr_lock::TryRead::Pending => {
                #[cfg(feature = "debug")]
                log::debug!("[{}] Read pending", index);
                return TryRead::Pending;
            }
        }
    }

    // To avoid the need for shared Arc references, clone and drop are written as methods instead of using std traits
    pub fn clone<T>(&self, buffer: &MpmcCircularBuffer<T>) -> Self {
        let (index, state) = loop {
            let index = self.index;
            let state = self.state;
            let slot = buffer.get_slot(index);

            let head_lock = buffer.head_lock.lock().unwrap();
            let head = buffer.head.load(Ordering::Acquire);

            if index >= head {
                slot.acquire_next();
            } else {
                slot.acquire();
            }

            drop(head_lock);

            break (index, state);
        };

        #[cfg(feature = "debug")]
        log::error!("[{}] Cloned reader, state {:?}", index, state);

        BufferReader { index, state }
    }

    pub fn drop<T>(&mut self, buffer: &MpmcCircularBuffer<T>) {
        let index = self.index;
        if let ReaderState::Blocked = self.state {
            #[cfg(feature = "debug")]
            log::error!("[{}] Dropping blocked reader", index);
            let head_lock = buffer.head_lock.lock().unwrap();
            let head = buffer.head.load(Ordering::Acquire);

            if index < head {
                drop(head_lock);
                // the head has advanced, and the reader is unblocked
                // decrement the read lock
                self.release(index, buffer);
            } else {
                // otherwise, decrement the lock on the next item in the slot
                let slot = buffer.get_slot(index);
                slot.decrement_next();
            }
        } else {
            #[cfg(feature = "debug")]
            log::error!("[{}] Dropping reader", index);
            self.release(index, buffer);
        }
    }

    fn advance<T>(&mut self, id: usize, buffer: &MpmcCircularBuffer<T>, cx: &Context<'_>) {
        let slot = buffer.get_slot(id);
        let next_id = id + 1;
        let next_slot = buffer.get_slot(next_id);

        let head_lock = buffer.head_lock.lock().unwrap();

        let head_id = buffer.head.load(Ordering::Acquire);

        if next_id >= head_id {
            next_slot.acquire_next();
            drop(head_lock);

            // if head_id != buffer.head.load(Ordering::Acquire) {
            //     next_slot.decrement_next();
            //     continue;
            // }

            self.state = ReaderState::Blocked;
            slot.subscribe_write(cx);
            #[cfg(feature = "debug")]
            log::info!("[{}] Reader blocked, head is {}", next_id, head_id);
        } else {
            buffer.get_slot(next_id).acquire();
            drop(head_lock);
        }
        // }

        // self.index.fetch_add(1, Ordering::AcqRel);
        self.index = next_id;
    }

    fn release<T>(&self, index: usize, buffer: &MpmcCircularBuffer<T>) {
        let slot = buffer.get_slot(index);
        if let TryDecrement::Dead = slot.decrement() {
            buffer.release_tail();
        }
    }
}
