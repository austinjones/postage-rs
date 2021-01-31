<img width=950 src="./readme/postage-banner.svg">

**The feature-rich, portable async channel library** \> **[crates.io](https://crates.io/crates/postage)** \> **[docs.rs](https://docs.rs/postage/)**

## Why use Postage?
- Includes a **rich set of channels.**
  **|** [barrier](https://docs.rs/postage/latest/postage/barrier/index.html)
  **|** [broadcast](https://docs.rs/postage/latest/postage/broadcast/index.html)
  **|** [dispatch](https://docs.rs/postage/latest/postage/dispatch/index.html) 
  **|** [mpsc](https://docs.rs/postage/latest/postage/mpsc/index.html) 
  **|** [oneshot](https://docs.rs/postage/latest/postage/oneshot/index.html) 
  **|** [watch](https://docs.rs/postage/latest/postage/watch/index.html) 
- Works with **any executor.**
  - Currently regressions are written for `tokio` and `async-std`.
- **Throughly tested.**  
  - Channels have full unit test coverage, and integration test coverage with multiple async executors.
- Includes **built-in [Stream](https://docs.rs/postage/latest/postage/stream/trait.Stream.html) and [Sink](https://docs.rs/postage/latest/postage/sink/trait.Sink.html) combinators.** 
  - Sinks can be chained and filtered.
  - Streams can be chained, filtered, mapped, and merged.
  - Sinks and streams can log their values, for easy app debugging.

Postage is in **beta** quality.  The functionality is implemented and has unit/integration test coverage.  But it needs to be tested on more hardware, and more operating systems.

## Channels
### postage::barrier
Barrier channels can be used to synchronize events, but do not transmit any data.  When the sender is dropped (or `tx.send(())` is called), the receiver is awoken.  This can be used to asynchronously coordinate actions between tasks.

### postage::broadcast
The broadcast channel provides reliable broadcast delivery between multiple senders and multiple receivers.  The channel has a fixed capacity, and senders are suspended if the buffer is filled.

When a receiver is cloned, both receivers will be sent the same series of messages.

Senders also provide a `subscribe()` method which creates a receiver that will observe all messages sent *after* the call to subscribe.

### postage::dispatch
The dispatch channel provides multi-sender, multi-receiver message dispatch.  A message will be observed by at most one reciever.  The channel has a fixed capacity, and senders are suspended if the buffer is filled.

Receivers can be created with `rx.clone()`, or `tx.subscribe()`.

### postage::mpsc
Postage includes a fixed-capacity multi-producer, single-consumer channel.  The producer can be cloned, and the sender task is suspended if the channel becomes full.

### postage::oneshot
Oneshot channels transmit a single value between a sender and a reciever.  Neither can be cloned.  If the sender drops, the receiver recieves a `None` value.

### postage::watch
Watch channels can be used to asynchronously transmit state.  When receivers are created, they immediately recieve an initial value.  They will also recieve new values, but are not guaranteed to recieve *every* value.

Values transmitted over watch channels must implement Default.  A simple way to achieve this is to transmit `Option<T>`.

## Benchmarks
Benchmarks of postage channels, and comparable async-std/tokio channels. 

- `send/recv` measures the total time to send and receive an item.
- `send full` measures the time to send an item and get a `Poll::Pending` value on a full channel.
- `recv empty` measures the time to get a `Poll::Pending` value on an empty channel.

All benchmarks were taken with criterion and are in the `benches` directory.

| Package   | Channel   | send/recv    | send full | recv empty |
| --------- | --------- | ------------ | --------- | ---------- |
| broadcast | postage   | 140ns        | 8ns       | 8ns        |
| broadcast | tokio     | 88ns (-38%)  | 49ns      | 39ns       |
| -         |           |              |           |            |
| dispatch  | postage   | 64ns         | 44ns      | 24ns       |
| dispatch  | async_std | 41ns (-36%)  | 10ns      | 10ns       |
| -         |           |              |           |            |
| mpsc      | postage   | 66ns         | 44ns      | 47ns       |
| mpsc      | tokio     | 128ns (+90%) | 1ns       | 31ns       |
| -         |           |              |           |            |
| watch     | postage   | 92ns         | -         | 7ns        |
| watch     | tokio     | 74ns (-20%)  | -         | 75ns       |
