use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub struct RefCount {
    count: AtomicUsize,
}

pub enum TryDecrement {
    Alive,
    Dead,
}

impl RefCount {
    pub fn new(count: usize) -> Self {
        Self {
            count: AtomicUsize::new(count),
        }
    }

    pub fn is_alive(&self) -> bool {
        self.count.load(Ordering::Acquire) > 0
    }

    pub fn load(&self, ordering: Ordering) -> usize {
        self.count.load(ordering)
    }

    pub fn take(&self) -> usize {
        self.count.swap(0, Ordering::AcqRel)
    }

    pub fn add(&self, n: usize) {
        self.count.fetch_add(n, Ordering::AcqRel);
    }

    pub fn increment(&self) {
        self.count.fetch_add(1, Ordering::AcqRel);
    }

    pub fn decrement(&self) -> TryDecrement {
        loop {
            let state = self.count.load(Ordering::Acquire);

            if state == 0 {
                return TryDecrement::Dead;
            }

            if let Ok(prev) =
                self.count
                    .compare_exchange(state, state - 1, Ordering::AcqRel, Ordering::Relaxed)
            {
                if state == 1 {
                    return TryDecrement::Dead;
                } else {
                    return TryDecrement::Alive;
                }
            }
        }
    }
}
