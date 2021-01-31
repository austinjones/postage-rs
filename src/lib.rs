//! **The feature-rich, portable async channel library.**
//!
//! # Why use Postage?
//! - Includes a **rich set of channels.**
//!   - `mpsc`, a multi-producer, single-consumer channel
//!   - `broadcast`, a lossless multi-producer, multi-consumer broadcast channel with backpressure (no lagging!)
//!   - `watch`, a state distribution channel with a value that can be borrowed.
//!   - `oneshot`, a oneshot transfer channel.
//!   - `barrier`, a oneshot channel that transmits when the sender half is dropped.
//! - Works with **any executor.**
//!   - Currently regressions are written for `tokio` and `async-std`.
//! - **Throughly tested.**  
//!   - Channels have full unit test coverage, and integration test coverage with multiple async executors.
//! - Comes with **built-in Stream and Sink combinators.**
//!   - Sinks can be chained, and filtered.
//!   - Streams can be chained, filtered, mapped, and merged.
//!   - With the `logging` feature, Sinks and streams can log their values.  This is really helpful when debugging applications.
//!
//! Postage is in *beta* quality.  The functionality is implemented and has unit/integration test coverage.  But it needs to be tested on more hardware, and more operating systems.
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
