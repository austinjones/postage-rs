use std::{
    cmp::max,
    sync::{atomic::AtomicUsize, RwLock},
};

use crate::Context;
use atomic::Ordering;

use super::notifier::Notifier;
use std::fmt::Debug;

// A lock-free multi-producer, multi-consumer circular buffer
// Each reader will see each value created exactly once.
// Cloned readers inherit the read location of the reader that was cloned.

pub struct MpmcCircularBuffer<T> {
    buffer: Box<[Slot<T>]>,
    head: AtomicUsize,
    maintenance: RwLock<()>,
    readers: AtomicUsize,
}

impl<T> Debug for MpmcCircularBuffer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MpmcCircularBuffer")
            .field("buffer", &self.buffer)
            .field("head", &self.head)
            .field("readers", &self.readers)
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
            vec.push(Slot::new(0));
        }

        let this = Self {
            buffer: vec.into_boxed_slice(),
            head: AtomicUsize::new(1),
            readers: AtomicUsize::new(1),
            maintenance: RwLock::new(()),
        };

        let reader = BufferReader { index: 1 };

        (this, reader)
    }
}

pub enum TryWrite<T> {
    Pending(T),
    Ready,
}

pub enum SlotTryWrite<T> {
    Pending(T),
    Ready,
    Written(T),
}

impl<T> MpmcCircularBuffer<T> {
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn try_write(&self, mut value: T, cx: &Context<'_>) -> TryWrite<T> {
        loop {
            let _maint = self.maintenance.read().unwrap();
            let head_id = self.head.load(Ordering::Acquire);
            let head_slot = self.get_slot(head_id);

            let readers = self.readers.load(Ordering::Acquire);

            #[cfg(feature = "debug")]
            log::debug!(
                "[{}] Attempting write with required readers {}, slot index {:?} with {:?} readers of {} required",
                head_id,
                readers,
                head_slot.index,
                head_slot.reads,
                readers
            );

            // try to write a value
            // if the write is accepted, release the head lock in the closure
            // this minimizes the time head is locked, and allows the move of value to occur after the lock is released
            let try_write = head_slot.try_write(head_id, value, readers, cx, || {
                if let Err(_e) = self.head.compare_exchange(
                    head_id,
                    head_id + 1,
                    Ordering::SeqCst,
                    Ordering::Relaxed,
                ) {
                    #[cfg(feature = "debug")]
                    log::warn!(
                        "[{}] Expected {} head value, found {}",
                        head_id,
                        head_id + 1,
                        _e
                    );
                }
            });

            match try_write {
                SlotTryWrite::Pending(v) => {
                    return TryWrite::Pending(v);
                }
                SlotTryWrite::Ready => {
                    #[cfg(feature = "debug")]
                    let slot_index = head_id % self.len();

                    #[cfg(feature = "debug")]
                    log::warn!(
                        "[{}] Write complete in slot {}, head incremented from {} to {}",
                        head_id,
                        slot_index,
                        head_id,
                        head_id + 1
                    );

                    return TryWrite::Ready;
                }
                SlotTryWrite::Written(v) => {
                    value = v;
                    continue;
                }
            }
        }
    }

    pub fn new_reader(&self) -> BufferReader {
        let _maint = self.maintenance.write().unwrap();
        let index = self.head.load(Ordering::Acquire);
        self.readers.fetch_add(1, Ordering::AcqRel);

        self.mark_read_in_range(0, index);

        #[cfg(feature = "debug")]
        log::info!("[{}] New reader", index);

        BufferReader { index }
    }

    fn mark_read_in_range(&self, min: usize, max: usize) {
        for slot in self.buffer.iter() {
            let readers = self.readers.load(Ordering::Acquire);
            slot.mark_read_in_range(min, max, readers);
        }
    }

    pub(in crate::sync::mpmc_circular_buffer) fn get_slot(&self, id: usize) -> &Slot<T> {
        let index = id % self.len();
        &self.buffer[index]
    }
}

#[derive(Debug)]
pub struct BufferReader {
    index: usize,
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
        let slot = buffer.get_slot(index);

        let readers = buffer.readers.load(Ordering::Acquire);

        let try_read = slot.try_read(index, readers, cx);

        match &try_read {
            TryRead::Ready(_) => {
                self.index += 1;

                #[cfg(feature = "debug")]
                log::debug!(
                    "[{}] Read complete in slot {} with {:?} reads of {} required",
                    index,
                    index % buffer.len(),
                    slot.reads,
                    readers,
                );
            }
            TryRead::Pending => {
                #[cfg(feature = "debug")]
                log::debug!("[{}] Read pending, slot: {:?}", index, slot);
            }
        }

        try_read
    }

    // To avoid the need for shared Arc references, clone and drop are written as methods instead of using std traits
    pub fn clone_with<T>(&self, buffer: &MpmcCircularBuffer<T>) -> Self {
        // let _head = buffer.head.read().unwrap();
        let _maint = buffer.maintenance.write().unwrap();
        buffer.readers.fetch_add(1, Ordering::AcqRel);

        let index = self.index;
        buffer.mark_read_in_range(0, index);

        #[cfg(feature = "debug")]
        log::error!("[{}] Cloned reader", index);

        BufferReader { index }
    }

    pub fn drop_with<T>(&mut self, buffer: &MpmcCircularBuffer<T>) {
        // let _head = buffer.head.read().unwrap();
        let _maint = buffer.maintenance.write().unwrap();
        buffer.readers.fetch_sub(1, Ordering::AcqRel);

        for (_id, slot) in buffer.buffer.iter().enumerate() {
            let readers = buffer.readers.load(Ordering::Acquire);

            #[cfg(feature = "debug")]
            log::debug!(
                "[{}] Dropping reader, notifying slot {} with reads {:?} of new reader count [{}/{:?}]",
                self.index,
                _id,
                slot.reads,
                readers,
                buffer.readers,
            );

            slot.decrement_read_in_range(0, self.index);

            slot.notify_readers_decreased(readers);
        }

        #[cfg(feature = "debug")]
        log::error!(
            "[{}] Dropped reader, readers reduced to {:?}",
            self.index,
            buffer.readers
        );
    }
}

pub struct Slot<T> {
    data: RwLock<Option<T>>,
    reads: AtomicUsize,
    index: AtomicUsize,
    on_write: Notifier,
    on_release: Notifier,
}

impl<T> Slot<T> {
    pub fn new(index: usize) -> Self {
        Self {
            data: RwLock::new(None),
            reads: AtomicUsize::new(0),
            index: AtomicUsize::new(index),
            on_write: Notifier::new(),
            on_release: Notifier::new(),
        }
    }

    pub fn try_write<OnWrite>(
        &self,
        index: usize,
        value: T,
        readers: usize,
        cx: &Context<'_>,
        on_write: OnWrite,
    ) -> SlotTryWrite<T>
    where
        OnWrite: FnOnce(),
    {
        loop {
            let prev_index = self.index.load(Ordering::Acquire);

            if prev_index >= index {
                return SlotTryWrite::Written(value);
            } else if prev_index != 0 && self.reads.load(Ordering::Acquire) < readers {
                self.on_release.subscribe(cx);

                if self.reads.load(Ordering::Acquire) >= readers {
                    #[cfg(feature = "debug")]
                    log::info!(
                        "[{}] Reads incremented during write, invalidating subscription",
                        index
                    );
                    continue;
                }

                return SlotTryWrite::Pending(value);
            }

            // lock the data, then update the index
            let mut data = self.data.write().unwrap();
            if let Err(_) =
                self.index
                    .compare_exchange(prev_index, index, Ordering::AcqRel, Ordering::Relaxed)
            {
                continue;
            }

            on_write();
            *data = Some(value);
            self.reads.store(0, Ordering::Release);
            self.on_write.notify();
            return SlotTryWrite::Ready;
        }
    }

    fn mark_read_in_range(&self, min: usize, max: usize, readers: usize) {
        // prevent the index from changing while maintenance is performed
        let _read = self.data.read().unwrap();
        let index = self.index.load(Ordering::Acquire);
        if index >= min && index < max {
            let reads = 1 + self.reads.fetch_add(1, Ordering::AcqRel);

            #[cfg(feature = "debug")]
            log::debug!(
                "[{}] Mark read in range occurred.  Increased reads to {} of required readers {}",
                index,
                reads,
                readers
            );

            if reads >= readers {
                self.on_release.notify();
            }
        }
    }

    fn decrement_read_in_range(&self, min: usize, max: usize) {
        // prevent the index from changing while maintenance is performed
        let _read = self.data.read().unwrap();
        let index = self.index.load(Ordering::Acquire);
        if index >= min && index < max {
            loop {
                let reads = self.reads.load(Ordering::Acquire);
                if reads == 0 {
                    return;
                }

                if let Ok(_) = self.reads.compare_exchange(
                    reads,
                    reads - 1,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    #[cfg(feature = "debug")]
                    log::debug!(
                        "[{}] Mark decrement in range occurred.  Decreased reads to {}",
                        index,
                        reads - 1
                    );

                    return;
                }
            }
        }
    }

    fn notify_readers_decreased(&self, readers: usize) {
        if self.reads.load(Ordering::Acquire) >= readers {
            self.on_release.notify();
        }
    }
}

impl<T> Slot<T>
where
    T: Clone,
{
    pub fn try_read(&self, index: usize, readers: usize, cx: &Context<'_>) -> TryRead<T> {
        let data = self.data.read().unwrap();
        if data.is_none() {
            self.on_write.subscribe(cx);
            drop(data);
            return TryRead::Pending;
        }

        let slot_index = self.index.load(Ordering::Acquire);
        if slot_index < index {
            self.on_write.subscribe(cx);
            return TryRead::Pending;
        } else if slot_index > index {
            panic!(
                "Slot index {} has advanced past reader position {}",
                slot_index, index
            );
        }

        let reads = 1 + self.reads.fetch_add(1, Ordering::AcqRel);
        #[cfg(feature = "debug")]
        log::debug!(
            "[{}] Read action occurred.  Increased reads to {}",
            index,
            reads
        );

        let data_ref = data.as_ref().unwrap();
        let data_cloned = data_ref.clone();
        // release the read lock

        if reads >= readers {
            self.on_release.notify();
        }

        TryRead::Ready(data_cloned)
    }
}

impl<T> Debug for Slot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slot")
            .field("reads", &self.reads)
            .field("index", &self.index)
            .finish()
    }
}
