use std::{collections::LinkedList, sync::Mutex, task::Waker};

#[derive(Debug)]
pub struct Notifier {
    wakers: Mutex<LinkedList<Waker>>,
}

impl Notifier {
    pub fn new() -> Self {
        Self {
            wakers: Mutex::new(LinkedList::new()),
        }
    }

    pub fn notify(&self) {
        let mut wakers = self.wakers.lock().unwrap();
        #[cfg(feature = "debug")]
        let mut woken = 0usize;

        while let Some(waker) = wakers.pop_back() {
            #[cfg(feature = "debug")]
            {
                woken += 1;
            }

            waker.wake();
        }

        #[cfg(feature = "debug")]
        log::debug!("Woke {} tasks", woken);
    }

    pub fn subscribe(&self, waker: Waker) {
        let mut wakers = self.wakers.lock().unwrap();
        wakers.push_front(waker);
    }
}
