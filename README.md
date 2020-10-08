# single_buffer_channel
Multi-producer, single-consumer single value communication primitives.

This crate provides a latest-message style channel, where update sources can update the latest value that the receiver owns it.

Unlike the `mpsc::channel` each value send will overwrite the 'latest' value.
This is useful, for example, when thread ***A*** is interested in the latest result of another continually working thread ***B***. ***B*** could update the channel with it's latest result each iteration, each new update becoming the new 'latest' value. Then ***A*** can own the latest value.

The internal sync primitives are private and essentially only lock over fast data moves.

# Examples
```rust
use single_buffer_channel::channel;

let (updater, receiver) = channel();

// Update the latest value.
updater.update(1).unwrap();
// recv() blocks current thread unless the latest value is updated.
assert_eq!(Ok(1), receiver.recv());

updater.update(10).unwrap();

// try_recv() does not block current thread.
assert_eq!(Ok(10), receiver.try_recv());
// try_recv() returns an error because the sender sends nothing yet from the time recv(), try_recv() or recv_timeout() was called.
assert!(receiver.try_recv().is_err());

// Updater can be cloned
let updater_another = updater.clone();

updater.update(2).unwrap();
updater_another.update(3).unwrap();

// Only the latest value can be received.
assert_eq!(Ok(3), receiver.recv());
```

# Related module and crate
- [std::sync::mpsc::channel](https://doc.rust-lang.org/std/sync/mpsc/index.html)
- [single_value_channel](https://crates.io/crates/single_value_channel)
