use std::task::Poll;

macro_rules! poll {
    ($self:ident, $cx:ident) => {{
        use crate::context::Context;
        use crate::prelude::Stream;

        let mut cx = Context::from_waker($cx.waker());

        return match $self.poll_recv(&mut cx) {
            crate::stream::PollRecv::Ready(v) => Poll::Ready(Some(v)),
            crate::stream::PollRecv::Pending => Poll::Pending,
            crate::stream::PollRecv::Closed => Poll::Ready(None),
        };
    }};
}

impl futures::stream::Stream for crate::barrier::Receiver {
    type Item = ();

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        poll!(self, cx)
    }
}

impl<T: Clone> futures::stream::Stream for crate::broadcast::Receiver<T> {
    type Item = T;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        poll!(self, cx)
    }
}

impl<T> futures::stream::Stream for crate::dispatch::Receiver<T> {
    type Item = T;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        poll!(self, cx)
    }
}

impl<T> futures::stream::Stream for crate::mpsc::Receiver<T> {
    type Item = T;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        poll!(self, cx)
    }
}

impl<T> futures::stream::Stream for crate::oneshot::Receiver<T> {
    type Item = T;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        poll!(self, cx)
    }
}

impl<T: Clone> futures::stream::Stream for crate::watch::Receiver<T> {
    type Item = T;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        poll!(self, cx)
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, task::Poll};

    use crate::{
        barrier, broadcast, dispatch, mpsc, oneshot,
        sink::{PollSend, Sink},
        watch,
    };
    use futures::Stream;

    macro_rules! test_stream {
        ($chan:expr, $val:expr) => {
            let mut std_cx = futures_test::task::noop_context();
            let mut cx = crate::test::noop_context();

            let (mut tx, mut rx) = $chan;
            assert_eq!(Poll::Pending, Pin::new(&mut rx).poll_next(&mut std_cx));

            assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, $val));

            assert_eq!(
                Poll::Ready(Some($val)),
                Pin::new(&mut rx).poll_next(&mut std_cx)
            );

            drop(tx);

            assert_eq!(Poll::Ready(None), Pin::new(&mut rx).poll_next(&mut std_cx));
        };
    }

    #[test]
    fn barrier() {
        let mut std_cx = futures_test::task::noop_context();
        let mut cx = crate::test::noop_context();

        let (mut tx, mut rx) = barrier::channel();
        assert_eq!(Poll::Pending, Pin::new(&mut rx).poll_next(&mut std_cx));

        assert_eq!(PollSend::Ready, Pin::new(&mut tx).poll_send(&mut cx, ()));

        assert_eq!(
            Poll::Ready(Some(())),
            Pin::new(&mut rx).poll_next(&mut std_cx)
        );

        drop(tx);

        assert_eq!(
            Poll::Ready(Some(())),
            Pin::new(&mut rx).poll_next(&mut std_cx)
        );
    }

    #[test]
    fn broadcast() {
        test_stream!(broadcast::channel(4), 1usize);
    }

    #[test]
    fn dispatch() {
        test_stream!(dispatch::channel(4), 1usize);
    }

    #[test]
    fn mpsc() {
        test_stream!(mpsc::channel(4), 1usize);
    }

    #[test]
    fn oneshot() {
        test_stream!(oneshot::channel(), 1usize);
    }

    #[test]
    fn watch() {
        let mut std_cx = futures_test::task::noop_context();
        let mut cx = crate::test::noop_context();

        let (mut tx, mut rx) = watch::channel();
        assert_eq!(
            Poll::Ready(Some(0usize)),
            Pin::new(&mut rx).poll_next(&mut std_cx)
        );

        assert_eq!(
            PollSend::Ready,
            Pin::new(&mut tx).poll_send(&mut cx, 1usize)
        );

        assert_eq!(
            Poll::Ready(Some(1usize)),
            Pin::new(&mut rx).poll_next(&mut std_cx)
        );

        drop(tx);

        assert_eq!(Poll::Ready(None), Pin::new(&mut rx).poll_next(&mut std_cx));
    }
}
