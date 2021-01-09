use atomic::{Atomic, Ordering};

use crate::{PollRecv, Stream};
use pin_project::pin_project;

#[derive(Copy, Clone)]
enum State {
    Left,
    Right,
}

impl State {
    pub fn swap(&self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

#[pin_project]
pub struct MergeStream<Left, Right> {
    state: Atomic<State>,
    #[pin]
    left: Left,
    #[pin]
    right: Right,
}

impl<Left, Right> MergeStream<Left, Right>
where
    Left: Stream,
    Right: Stream<Item = Left::Item>,
{
    pub fn new(left: Left, right: Right) -> Self {
        Self {
            state: Atomic::new(State::Left),
            left,
            right,
        }
    }
}

impl<Left, Right> Stream for MergeStream<Left, Right>
where
    Left: Stream,
    Right: Stream<Item = Left::Item>,
{
    type Item = Left::Item;

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut futures_task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        let this = self.project();

        let state = this.state.load(Ordering::Acquire);
        let poll = match state {
            State::Left => this.left.poll_recv(cx),
            State::Right => this.right.poll_recv(cx),
        };

        match poll {
            PollRecv::Ready(v) => {
                this.state.store(state.swap(), Ordering::Release);
                PollRecv::Ready(v)
            }
            PollRecv::Pending => PollRecv::Pending,
            PollRecv::Closed => PollRecv::Closed,
        }
    }
}
