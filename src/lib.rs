//! **The feature-rich, portable async channel library.**
//!
//! # Why use Postage?
//! - Includes a **rich set of channels.**
//!   - `barrier`, a oneshot channel that transmits when the sender half is dropped.
//!   - `broadcast`, a lossless multi-producer, multi-consumer broadcast channel with backpressure (no lagging!)
//!   - `dispatch`, a multi-producer, multi-consumer queue
//!   - `mpsc`, a multi-producer, single-consumer channel
//!   - `oneshot`, a oneshot transfer channel.
//!   - `watch`, a state distribution channel with a value that can be borrowed.
//! - Works with **any executor.**
//!   - Currently regressions are written for `tokio` and `async-std`.
//! - **Throughly tested.**  
//!   - Channels have full unit test coverage, and integration test coverage with multiple async executors.
//! - Comes with **built-in Stream and Sink combinators.**
//!   - Sinks can be chained, and filtered.
//!   - Streams can be chained, filtered, mapped, and merged.
//!   - With the `logging` feature, Sinks and streams can log their values.  This is really helpful when debugging applications.
//!
//! See [the readme](https://github.com/austinjones/postage-rs#benchmarks) for benchmarks.

mod channels;
mod context;
mod logging;
pub mod prelude;
pub mod sink;
pub mod stream;
mod sync;

pub use channels::barrier;
pub use channels::broadcast;
pub use channels::dispatch;
pub use channels::mpsc;
pub use channels::oneshot;
pub use channels::watch;

pub use context::Context;

#[cfg(test)]
mod test;
