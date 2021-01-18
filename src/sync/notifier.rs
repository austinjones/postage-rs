use crossbeam_queue::SegQueue;
use std::task::Waker;

#[derive(Debug)]
pub struct Notifier {
    wakers: SegQueue<Waker>,
}

impl Notifier {
    pub fn new() -> Self {
        Self {
            wakers: SegQueue::new(),
        }
    }

    pub fn notify(&self) {
        #[cfg(feature = "debug")]
        let mut woken = 0usize;

        while let Some(waker) = self.wakers.pop() {
            #[cfg(feature = "debug")]
            {
                woken += 1;
            }

            waker.wake();
        }

        #[cfg(feature = "debug")]
        if woken > 0 {
            log::info!("Woke {} tasks", woken);
        }
    }

    pub fn subscribe(&self, cx: &crate::Context) {
        if let Some(waker) = cx.waker() {
            self.wakers.push(waker);
        }
    }
}
