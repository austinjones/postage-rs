# postage-rs
The feature-rich, portable async channel library.

Postage is in *beta* quality.  The functionality is implemented and tested, but needs to be tested on more hardware, and more operating systems.

Why use Postage?
- Includes a **rich set of channels**, and it works with any async executor (currently regressions are written for tokio and async-std)
  - `mpsc`, a multi-producer, single-consumer channel
  - `broadcast`, a multi-producer, multi-consumer broadcast channel with backpressure (no lagging!)
  - `watch`, a stateful channel where receivers receive an initial value, and updates when the value state changes.
  - `oneshot`, a transfer channel that can be used once.
  - `barrier`, a channel that doesn't carry a value, but transmits when the sender half is dropped.
- Comes with **built-in Stream and Sink combinators**.
  - Sinks can be chained, and filtered.
  - Streams can be chained, filtered, mapped, and merged.
  - Sinks and streams can log their values, for easy app debugging.

## Channels
### postage::mpsc
Postage includes a fixed-capacity multi-producer, single-consumer channel.  The producer can be cloned, and the sender task is suspended if the channel becomes full.

### postage::broadcast
The broadcast channel provides multi-sender, multi-receiver message dispatch.  All receivers are sent every message.  The channel has a fixed capacity, and senders are suspended if the buffer is filled.

When a receiver is cloned, both receivers will be sent the same series of messages.

Senders also provide a `subscribe()` method which adds a receiver on the oldest value.

### postage::watch
Watch channels can be used to asynchronously transmit state.  When receivers are created, they immediately recieve an initial value.  They will also recieve new values, but are not guaranteed to recieve *every* value.

Values transmitted over watch channels must implement Default.  A simple way to achieve this is to transmit `Option<T>`.

### postage::oneshot
Oneshot channels transmit a single value between a sender and a reciever.  Neither can be cloned.  If the sender drops, the receiver recieves a `None` value.

### postage::barrier
Barrier channels can be used to synchronize events, but do not transmit any data.  When the sender is dropped (or `tx.send(())` is called), the receiver is awoken.  This can be used to asynchronously coordinate actions between tasks.
