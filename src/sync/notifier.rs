use std::{collections::LinkedList, sync::Mutex, task::Waker};

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

        while let Some(waker) = wakers.pop_back() {
            waker.wake();
        }
    }

    pub fn subscribe(&self, waker: Waker) {
        let mut wakers = self.wakers.lock().unwrap();
        wakers.push_front(waker);
    }
}
