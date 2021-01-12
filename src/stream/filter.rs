use std::{pin::Pin, task::Context};

use crate::{PollRecv, Stream};

pub struct FilterStream<From, Filter> {
    from: From,
    filter: Filter,
}

impl<From, Filter> FilterStream<From, Filter>
where
    From: Stream + Unpin,
    Filter: FnMut(&From::Item) -> bool + Unpin,
{
    pub fn new(from: From, filter: Filter) -> Self {
        Self { from, filter }
    }
}

impl<From, Filter> Stream for FilterStream<From, Filter>
where
    From: Stream + Unpin,
    Filter: FnMut(&From::Item) -> bool + Unpin,
{
    type Item = From::Item;

    fn poll_recv(self: Pin<&mut Self>, cx: &mut Context<'_>) -> crate::PollRecv<Self::Item> {
        let this = self.get_mut();
        loop {
            let from = Pin::new(&mut this.from);
            match from.poll_recv(cx) {
                PollRecv::Ready(value) => {
                    if !(this.filter)(&value) {
                        return PollRecv::Ready(value);
                    }
                }
                PollRecv::Pending => return PollRecv::Pending,
                PollRecv::Closed => return PollRecv::Closed,
            }
        }
    }
}
