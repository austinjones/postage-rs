use std::pin::Pin;

use atomic::{Atomic, Ordering};

use crate::Context;
use crate::{PollRecv, Stream};
use pin_project::pin_project;

#[derive(Copy, Clone)]
enum State {
    Left,
    Right,
    Closed,
}

#[pin_project]
pub struct ChainStream<Left, Right> {
    state: Atomic<State>,
    #[pin]
    left: Left,
    #[pin]
    right: Right,
}

impl<Left, Right> ChainStream<Left, Right>
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

impl<Left, Right> Stream for ChainStream<Left, Right>
where
    Left: Stream,
    Right: Stream<Item = Left::Item>,
{
    type Item = Left::Item;

    fn poll_recv(self: Pin<&mut Self>, cx: &mut Context<'_>) -> crate::PollRecv<Self::Item> {
        let this = self.project();
        let mut state = this.state.load(Ordering::Acquire);

        if let State::Left = state {
            match this.left.poll_recv(cx) {
                PollRecv::Ready(v) => return PollRecv::Ready(v),
                PollRecv::Pending => return PollRecv::Pending,
                PollRecv::Closed => {
                    this.state.store(State::Right, Ordering::Release);
                    state = State::Right;
                }
            }
        }

        if let State::Right = state {
            match this.right.poll_recv(cx) {
                PollRecv::Ready(v) => return PollRecv::Ready(v),
                PollRecv::Pending => return PollRecv::Pending,
                PollRecv::Closed => {
                    this.state.store(State::Closed, Ordering::Release);
                    return PollRecv::Closed;
                }
            }
        }

        if let State::Closed = state {
            return PollRecv::Closed;
        }

        unreachable!();
    }
}
