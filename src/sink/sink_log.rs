use crate::{PollSend, Sink};
use log::log_enabled;
use pin_project::pin_project;
use std::{fmt::Debug, pin::Pin};

use crate::Context;
#[pin_project]
pub struct SinkLog<S> {
    #[pin]
    sink: S,
    level: log::Level,
}

impl<S> SinkLog<S> {
    pub fn new(sink: S, level: log::Level) -> Self {
        SinkLog { sink, level }
    }
}

impl<S> Sink for SinkLog<S>
where
    S: Sink,
    S::Item: Debug,
{
    type Item = S::Item;

    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        value: Self::Item,
    ) -> crate::PollSend<Self::Item> {
        let this = self.project();
        let level = *this.level;

        let debug_repr = if log_enabled!(level) {
            Some(format!("{:?}", &value))
        } else {
            None
        };

        match this.sink.poll_send(cx, value) {
            PollSend::Ready => {
                if let Some(msg) = debug_repr {
                    log::log!(level, "{}", msg);
                }
                PollSend::Ready
            }
            PollSend::Pending(v) => PollSend::Pending(v),
            PollSend::Rejected(v) => PollSend::Rejected(v),
        }
    }
}
