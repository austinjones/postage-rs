use std::pin::Pin;

use crate::Context;

use crate::{PollSend, Sink};
use pin_project::pin_project;

#[pin_project]
pub struct SinkFilter<Filter, Into> {
    filter: Filter,
    #[pin]
    into: Into,
}

impl<Filter, Into> SinkFilter<Filter, Into>
where
    Into: Sink,
    Filter: FnMut(&Into::Item) -> bool,
{
    pub fn new(filter: Filter, into: Into) -> Self {
        Self { filter, into }
    }
}

impl<Filter, Into> Sink for SinkFilter<Filter, Into>
where
    Into: Sink,
    Filter: FnMut(&Into::Item) -> bool,
{
    type Item = Into::Item;

    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        value: Self::Item,
    ) -> crate::PollSend<Self::Item> {
        let this = self.project();
        if !(this.filter)(&value) {
            return PollSend::Ready;
        }

        this.into.poll_send(cx, value)
    }
}
