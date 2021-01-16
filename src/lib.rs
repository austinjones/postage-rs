//! The feature-rich, portable async channel library.
//!
//! Provides a set of async channels that can be used with any async executor.
mod channels;
mod sink;
mod stream;
mod sync;

pub use channels::barrier;
pub use channels::broadcast;
pub use channels::mpsc;
pub use channels::oneshot;
pub use channels::watch;

pub use sink::*;
pub use stream::*;

#[cfg(test)]
mod test;
