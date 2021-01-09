use std::sync::atomic::{AtomicUsize, Ordering};

pub struct RefCount {
    count: AtomicUsize,
}

impl RefCount {
    pub fn new(count: usize) -> Self {
        Self {
            count: AtomicUsize::new(count),
        }
    }

    pub fn is_alive(&self) -> bool {
        self.count.load(Ordering::SeqCst) > 0
    }

    pub fn increment(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn decrement(&self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }
}
