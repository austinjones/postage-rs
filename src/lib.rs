//! The feature-rich, portable async channel library. Provides a set of async channels that can be used with any async executor.
//!
//! Why use Postage?
//! - Includes a **rich set of channels**, and it works with any async executor (currently regressions are written for tokio and async-std)
//!   - `mpsc`, a multi-producer, single-consumer channel
//!   - `broadcast`, a multi-producer, multi-consumer broadcast channel with backpressure (no lagging!)
//!   - `watch`, a stateful channel where receivers receive an initial value, and updates when the value state changes.
//!   - `oneshot`, a transfer channel that can be used once.
//!   - `barrier`, a channel that doesn't carry a value, but transmits when the sender half is dropped.
//! - Comes with **built-in Stream and Sink combinators**.
//!   - Sinks can be chained, and filtered.
//!   - Streams can be chained, filtered, mapped, and merged.
//!   - Sinks and streams can log their values, for easy app debugging.
//!
//! Postage is in *beta* quality.  The functionality is implemented and tested, but needs to be tested on more hardware, and more operating systems.

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
