use crate::Sink;

pub struct TrySend<S: Sink> {
    send: S,
}
