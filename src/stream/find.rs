use std::pin::Pin;

use atomic::{Atomic, Ordering};

use crate::{PollRecv, Stream};

#[derive(Copy, Clone)]
enum State {
    Reading,
    Closed,
}

pub struct FindStream<From, Condition> {
    state: Atomic<State>,
    from: From,
    condition: Condition,
}

impl<From, Condition> FindStream<From, Condition> {
    pub fn new(from: From, condition: Condition) -> Self {
        Self {
            state: Atomic::new(State::Reading),
            from,
            condition,
        }
    }
}

impl<From, Condition> Stream for FindStream<From, Condition>
where
    From: Stream + Unpin,
    Condition: Fn(&From::Item) -> bool + Unpin,
{
    type Item = From::Item;

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut futures_task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        let this = self.get_mut();

        if let State::Closed = this.state.load(Ordering::Acquire) {
            return PollRecv::Closed;
        }

        loop {
            let from = Pin::new(&mut this.from);
            match from.poll_recv(cx) {
                PollRecv::Ready(value) => {
                    if !(this.condition)(&value) {
                        this.state.store(State::Closed, Ordering::Release);

                        return PollRecv::Ready(value);
                    }
                }
                PollRecv::Pending => return PollRecv::Pending,
                PollRecv::Closed => return PollRecv::Closed,
            }
        }
    }
}
