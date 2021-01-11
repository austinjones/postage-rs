use std::{cell::UnsafeCell, sync::atomic::AtomicUsize};

use atomic::{Atomic, Ordering};

use super::ref_count::{RefCount, TryDecrement};

// A lock-free multi-producer, multi-consumer circular buffer
// Each reader will see each value created exactly once.
// Cloned readers inherit the read location of the reader that was cloned.

pub struct MpmcCircularBuffer<T> {
    buffer: Box<[Slot<T>]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl<T> MpmcCircularBuffer<T>
where
    T: Clone,
{
    pub fn new(capacity: usize) -> (Self, BufferReader) {
        // add one capacity so that readers have an empty slot to pause
        let mut vec = Vec::with_capacity(capacity + 1);

        for _ in 0..(capacity + 1) {
            vec.push(Slot::new());
        }

        let this = Self {
            buffer: vec.into_boxed_slice(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        };

        let reader = BufferReader {
            index: AtomicUsize::new(0),
        };

        this.buffer[0].add_reader();

        (this, reader)
    }
}

pub enum TryWrite<T> {
    Pending(T),
    Ready,
}

impl<T> MpmcCircularBuffer<T> {
    pub fn try_write(&self, mut value: T) -> TryWrite<T> {
        loop {
            let head_id = self.head.load(Ordering::Acquire);
            let next_slot_id = self.index(head_id + 1);
            let next_slot = self.get(next_slot_id);

            // keep the next slot free, so readers have a location to pause if the buffer fills
            match next_slot.state.load(Ordering::Acquire) {
                SlotState::None => {}
                SlotState::Writing | SlotState::Reading => {
                    return TryWrite::Pending(value);
                }
            }

            let head_slot = self.get(head_id);

            // println!("Write slot: {:?}", head_slot);

            match head_slot.write(value) {
                Ok(_) => {
                    // TOOD: remove unwrap
                    self.head
                        .compare_exchange(
                            head_id,
                            next_slot_id,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        )
                        .unwrap();

                    return TryWrite::Ready;
                }
                Err((slot_state, returned_value)) => match slot_state {
                    SlotState::None => unreachable!(),
                    SlotState::Writing => {
                        value = returned_value;
                        continue;
                    }
                    SlotState::Reading => {
                        return TryWrite::Pending(returned_value);
                    }
                },
            }
        }
    }

    pub(in crate::sync::mpmc_circular_buffer) fn index(&self, location: usize) -> usize {
        location % self.buffer.len()
    }

    pub(in crate::sync::mpmc_circular_buffer) fn get(&self, index: usize) -> &Slot<T> {
        &self.buffer[index]
    }
}

pub struct BufferReader {
    index: AtomicUsize,
}

pub enum TryRead<T> {
    /// A value is pending in this slot
    Pending,
    /// A value was read, but other readers are pending
    Reading(T),
    /// A value was read, and this was the final reader.
    Complete(T),
}

impl BufferReader {
    pub fn try_read<T>(&self, buffer: &MpmcCircularBuffer<T>) -> TryRead<T>
    where
        T: Clone,
    {
        loop {
            let slot_id = self.index.load(Ordering::Acquire);
            let slot = buffer.get(slot_id);
            let next_slot_id = buffer.index(slot_id + 1);
            let next_slot = buffer.get(next_slot_id);

            match slot.state.load(Ordering::Acquire) {
                SlotState::None | SlotState::Writing => return TryRead::Pending,
                SlotState::Reading => {
                    debug_assert!(slot.readers.load(Ordering::Acquire) > 0);
                    if let Err(_) = self.index.compare_exchange(
                        slot_id,
                        slot_id + 1,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    ) {
                        // another thread has read the value
                        // let's try the next one
                        continue;
                    }

                    next_slot.add_reader();

                    match self.try_release(slot_id, buffer) {
                        TryRelease::Released => {
                            let value = unsafe { slot.clone_value() };
                            return TryRead::Reading(value);
                        }
                        TryRelease::Complete => {
                            let value = unsafe { slot.take_value() };
                            slot.state.store(SlotState::None, Ordering::Release);

                            return TryRead::Complete(value);
                        }
                    }
                }
            }
        }
    }

    // To avoid the need for shared Arc references, clone and drop are written as methods instead of using std traits
    pub fn clone<T>(&self, buffer: &MpmcCircularBuffer<T>) -> Self {
        let index = self.index.load(Ordering::Acquire);

        let slot = buffer.get(index);
        slot.add_reader();

        BufferReader {
            index: AtomicUsize::new(index),
        }
    }

    pub fn drop<T>(&mut self, buffer: &MpmcCircularBuffer<T>) {
        let id = self.index.load(Ordering::Acquire);

        match self.try_release(id, buffer) {
            TryRelease::Released => {}
            TryRelease::Complete => {
                let slot = buffer.get(id);
                slot.state.store(SlotState::None, Ordering::Release);
            }
        }
    }

    fn try_release<T>(&self, id: usize, buffer: &MpmcCircularBuffer<T>) -> TryRelease {
        let slot = buffer.get(id);

        if id == buffer.tail.load(Ordering::Acquire) {
            match slot.readers.decrement() {
                TryDecrement::Alive => {}
                TryDecrement::Dead => {
                    let next_id = buffer.index(id + 1);

                    // todo: figure out how to remove unwrap
                    buffer
                        .tail
                        .compare_exchange(id, next_id, Ordering::AcqRel, Ordering::Relaxed)
                        .unwrap();

                    return TryRelease::Complete;
                }
            }
        } else {
            slot.readers.decrement();
        }

        TryRelease::Released
    }
}

enum TryRelease {
    /// The slot was released
    Released,
    /// This was the final reader in the tail position
    /// The caller needs to take the element, and set the state to None
    Complete,
}

#[derive(Debug)]
struct Slot<T> {
    value: UnsafeCell<Option<T>>,
    state: Atomic<SlotState>,
    readers: RefCount,
}

unsafe impl<T> Sync for Slot<T> where T: Clone + Send {}

impl<T> Slot<T> {
    pub fn new() -> Self {
        Self {
            value: UnsafeCell::new(None),
            state: Atomic::new(SlotState::None),
            readers: RefCount::new(0),
        }
    }

    pub fn add_reader(&self) {
        self.readers.increment();
    }

    pub fn write(&self, value: T) -> Result<(), (SlotState, T)> {
        if self.readers.load(Ordering::Acquire) == 0 {
            self.state.store(SlotState::None, Ordering::Release);
        }

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

        self.state.store(SlotState::Reading, Ordering::Release);
        Ok(())
    }
}

impl<T> Slot<T>
where
    T: Clone,
{
    pub unsafe fn clone_value(&self) -> T {
        let reference = self.value.get();
        let r = reference.as_ref().unwrap();
        r.as_ref().unwrap().clone()
    }

    pub unsafe fn take_value(&self) -> T {
        let reference = self.value.get();
        let r = reference.as_mut().unwrap();
        r.take().unwrap()
    }
}

#[derive(Copy, Clone, Debug)]
enum SlotState {
    None,
    Writing,
    Reading,
}
