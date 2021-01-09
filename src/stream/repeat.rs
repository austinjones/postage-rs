use crate::{PollRecv, Stream};

pub struct RepeatStream<T> {
    data: T,
}

impl<T> RepeatStream<T>
where
    T: Clone,
{
    pub fn new(data: T) -> Self {
        Self { data }
    }
}

impl<T> Stream for RepeatStream<T>
where
    T: Clone,
{
    type Item = T;

    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut futures_task::Context<'_>,
    ) -> crate::PollRecv<Self::Item> {
        return PollRecv::Ready(self.data.clone());
    }
}
