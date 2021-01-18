use crate::Context;
use pin_project::pin_project;
use std::{fmt::Debug, pin::Pin};

use log::log;

use super::{PollRecv, Stream};
#[pin_project]
pub struct StreamLog<S> {
    #[pin]
    stream: S,
    level: log::Level,
}

impl<S> StreamLog<S> {
    pub fn new(stream: S, level: log::Level) -> Self {
        StreamLog { stream, level }
    }
}

impl<S> Stream for StreamLog<S>
where
    S: Stream,
    S::Item: Debug,
{
    type Item = S::Item;

    fn poll_recv(self: Pin<&mut Self>, cx: &mut Context<'_>) -> PollRecv<Self::Item> {
        let this = self.project();
        match this.stream.poll_recv(cx) {
            PollRecv::Ready(value) => {
                log!(*this.level, "{:?}", &value);
                PollRecv::Ready(value)
            }
            PollRecv::Pending => PollRecv::Pending,
            PollRecv::Closed => PollRecv::Closed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use log::Level;

    use crate::test::stream::*;
    use crate::{
        stream::{PollRecv, Stream},
        Context,
    };

    use super::StreamLog;

    #[test]
    fn ready_forwarded() {
        let mut repeat = StreamLog::new(ready(1usize), Level::Info);
        let mut cx = Context::empty();

        assert_eq!(PollRecv::Ready(1), Pin::new(&mut repeat).poll_recv(&mut cx));
    }

    #[test]
    fn pending_forwarded() {
        let mut repeat = StreamLog::new(pending::<usize>(), Level::Info);
        let mut cx = Context::empty();

        assert_eq!(PollRecv::Pending, Pin::new(&mut repeat).poll_recv(&mut cx));
    }

    #[test]
    fn closed_forwarded() {
        let mut repeat = StreamLog::new(closed::<usize>(), Level::Info);
        let mut cx = Context::empty();

        assert_eq!(PollRecv::Closed, Pin::new(&mut repeat).poll_recv(&mut cx));
    }
}
