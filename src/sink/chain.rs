use crate::{PollSend, Sink};
use atomic::{Atomic, Ordering};
use pin_project::pin_project;
use std::{pin::Pin, task::Context};

#[derive(Copy, Clone)]
enum State {
    WritingLeft,
    WritingRight,
    Closed,
}
#[pin_project]
pub struct SinkChain<Left, Right> {
    state: Atomic<State>,

    #[pin]
    left: Left,
    #[pin]
    right: Right,
}

impl<Left, Right> SinkChain<Left, Right>
where
    Left: Sink,
    Right: Sink<Item = Left::Item>,
{
    pub fn new(left: Left, right: Right) -> Self {
        Self {
            state: Atomic::new(State::WritingLeft),
            left,
            right,
        }
    }
}

impl<Left, Right> Sink for SinkChain<Left, Right>
where
    Left: Sink,
    Right: Sink<Item = Left::Item>,
{
    type Item = Left::Item;

    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut value: Self::Item,
    ) -> crate::PollSend<Self::Item> {
        let this = self.project();
        let mut state = this.state.load(Ordering::Acquire);

        if let State::WritingLeft = state {
            match this.left.poll_send(cx, value) {
                PollSend::Ready => return PollSend::Ready,
                PollSend::Pending(value) => return PollSend::Pending(value),
                PollSend::Rejected(returned_value) => {
                    value = returned_value;
                    this.state.store(State::WritingRight, Ordering::Release);
                    state = State::WritingRight;
                }
            }
        }

        if let State::WritingRight = state {
            match this.right.poll_send(cx, value) {
                PollSend::Ready => return PollSend::Ready,
                PollSend::Pending(value) => return PollSend::Pending(value),
                PollSend::Rejected(returned_value) => {
                    value = returned_value;

                    this.state.store(State::Closed, Ordering::Release);
                    return PollSend::Rejected(value);
                }
            }
        }

        if let State::Closed = state {
            return PollSend::Rejected(value);
        }

        unreachable!();
    }
}
