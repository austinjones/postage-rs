use std::marker::PhantomData;

use crate::{PollRecv, Stream};
use pin_project::pin_project;

#[pin_project]
pub struct MapStream<From, Map, Into> {
    #[pin]
    from: From,

    map: Map,
    into: PhantomData<Into>,
}

impl<From, Map, Into> MapStream<From, Map, Into>
where
    From: Stream,
    Map: Fn(From::Item) -> Into,
{
    pub fn new(from: From, map: Map) -> Self {
        Self {
            from,
            map,
            into: PhantomData,
        }
    }
}

impl<From, Map, Into> Stream for MapStream<From, Map, Into>
where
    From: Stream,
    Map: Fn(From::Item) -> Into,
{
    type Item = Into;

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut futures_task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        let this = self.project();

        match this.from.poll_recv(cx) {
            PollRecv::Ready(v) => PollRecv::Ready((this.map)(v)),
            PollRecv::Pending => PollRecv::Pending,
            PollRecv::Closed => PollRecv::Closed,
        }
    }
}

// impl<From, Map, Sink> MapSink<From, Map, Sink> {
//     pub fn new(map: Map, sink: Sink) {
//         Self {
//             from: PhandomData
//             map,
//             into: PhantomData,
//         }
//     }
// }
