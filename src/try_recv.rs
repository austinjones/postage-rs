use crate::Stream;

pub struct TryRecv<R: Stream> {
    recv: R,
}
