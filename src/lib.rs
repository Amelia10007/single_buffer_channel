//! Multi-producer, single-consumer single value communication primitives.
//!
//! This crate provides a latest-message style channel,
//! where update sources can update the latest value that the receiver owns it.
//!
//! Unlike the `mpsc::channel` each value send will overwrite the 'latest' value.
//!
//! This is useful, for example, when thread ***A*** is interested in the latest result of another
//! continually working thread ***B***. ***B*** could update the channel with it's latest result
//! each iteration, each new update becoming the new 'latest' value. Then ***A*** can own the latest value.
//!
//! The internal sync primitives are private and essentially only lock over fast data moves.
//!
//! # Examples
//! ```
//! use single_buffer_channel::channel;
//!
//! let (updater, receiver) = channel();
//!
//! // Update the latest value.
//! updater.update(1).unwrap();
//! // recv() blocks current thread unless the latest value is updated.
//! assert_eq!(Ok(1), receiver.recv());
//!
//! updater.update(10).unwrap();
//!
//! // try_recv() does not block current thread.
//! assert_eq!(Ok(10), receiver.try_recv());
//! // try_recv() returns an error because the sender sends nothing yet from the time recv(), try_recv() or recv_timeout() was called.
//! assert!(receiver.try_recv().is_err());
//!
//! // Updater can be cloned
//! let updater_another = updater.clone();
//!
//! updater.update(2).unwrap();
//! updater_another.update(3).unwrap();
//!
//! // Only the latest value can be received.
//! assert_eq!(Ok(3), receiver.recv());
//! ```

use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

/// The receiving-half of the single value channel.
/// Values sent to the channel can be retrieved using `recv`, `try_recv` and `recv_timeout`.
#[derive(Debug)]
pub struct Receiver<T> {
    latest: Arc<Mutex<Option<T>>>,
}

impl<T> Receiver<T> {
    /// Waits the latest value of this channel.
    /// # Returns
    /// `Ok()` if the latest value can be retrieved.
    /// `Err()` if no updater of this channel exists.
    ///
    /// # Examples
    /// ```
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// // Receive the latest sent value.
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.recv());
    ///
    /// // Nothing never received because the updater does not exist.
    /// drop(updater);
    /// assert!(receiver.recv().is_err());
    /// ```
    pub fn recv(&self) -> Result<T, RecvError> {
        loop {
            match self.try_recv() {
                Ok(value) => break Ok(value),
                Err(TryRecvError::Disconnected) => break Err(RecvError),
                _ => {}
            }
        }
    }

    /// Attempts to receive the latest value of this channel.
    ///
    /// After successful reception, nothing can be retrieved unless updater(s) sends a next value.
    /// # Returns
    /// `Ok()` if the latest value can be retrieved.
    /// `Err()` if any condition satisfies:
    /// - no updater of this channel exists.
    /// - an updater of this channel is sending the latest value.
    /// - nothing has been sent from the last reception.
    ///
    /// # Examples
    /// ```
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    /// // Nothing sent yet.
    /// assert!(receiver.try_recv().is_err());
    ///
    /// // Receive the latest sent value.
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.try_recv());
    ///
    /// // Nothing sent yet from the last reception.
    /// assert!(receiver.try_recv().is_err());
    /// ```
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        if !self.has_updater() {
            Err(TryRecvError::Disconnected)
        } else {
            // lock() fails if another owner of the `Mutex` has panicked during holding the mutex.
            // But the owners (`Updater`) never panic at the time.
            // Therefore, this unwrap() always succeeds.
            match self.latest.lock().unwrap().take() {
                Some(value) => Ok(value),
                None => Err(TryRecvError::Empty),
            }
        }
    }

    /// Waits the latest value of this channel is sent until the specified duration.
    ///
    /// After successful reception, nothing can be retrieved unless updater(s) sends a next value.
    /// # Returns
    /// `Ok()` if the latest value can be retrieved.
    /// `Err()` if any condition satisfies:
    /// - no updater of this channel exists.
    /// - nothing has been sent for the specified duration.
    ///
    /// # Examples
    /// ```
    /// use std::time::Duration;
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// // Receive the latest sent value.
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.recv_timeout(Duration::from_secs(1)));
    ///
    /// // Timeout because nothing has been sent for 1 second.
    /// assert!(receiver.recv_timeout(Duration::from_secs(1)).is_err());
    /// ```
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                break Err(RecvTimeoutError::Timeout);
            }
            match self.try_recv() {
                Ok(value) => break Ok(value),
                Err(TryRecvError::Disconnected) => break Err(RecvTimeoutError::Disconnected),
                _ => {}
            }
        }
    }

    /// Returns `true` if there is at least 1 updater of this channel.
    fn has_updater(&self) -> bool {
        Arc::weak_count(&self.latest) != 0
    }
}

/// An error returned from the `recv` function on a `Receiver`.
///
/// This error occurs if the sender(s) has become disconnected.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RecvError;

impl Display for RecvError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "The sender(s) has become disconnected, and there will never be any more data received on it.")
    }
}

impl Error for RecvError {}

/// An error returned from the `try_recv` function on a `Receiver`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TryRecvError {
    /// The sender(s) has become disconnected, and there will never be any more data received on it.
    Disconnected,
    /// This channel is currently empty, but the updater(s) have not yet disconnected, so data may yet become available.
    Empty,
}

impl Display for TryRecvError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let msg = match self {
            TryRecvError::Disconnected => "The sender(s) has become disconnected, and there will never be any more data received on it.",
            TryRecvError::Empty => "This channel is currently empty, but the updater(s) have not yet disconnected, so data may yet become available.",
        };
        write!(f, "{}", msg)
    }
}

impl Error for TryRecvError {}

/// An error returned from the `recv_timeout` function on a `Receiver`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RecvTimeoutError {
    /// This channel is currently empty, but the updater(s) have not yet disconnected, so data may yet become available.
    Timeout,
    /// The sender(s) has become disconnected, and there will never be any more data received on it.
    Disconnected,
}

impl Display for RecvTimeoutError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let msg = match self {
            RecvTimeoutError::Timeout => "This channel is currently empty, but the updater(s) have not yet disconnected, so data may yet become available.",
            RecvTimeoutError::Disconnected => "The updater(s) has become disconnected, and there will never be any more data received on it.",
        };
        write!(f, "{}", msg)
    }
}

impl Error for RecvTimeoutError {}

/// The updating-half of the single value channel.
#[derive(Debug, Clone)]
pub struct Updater<T> {
    destination: Weak<Mutex<Option<T>>>,
}

impl<T> Updater<T> {
    /// Updates the latest value of this channel.
    /// # Returns
    /// `Ok()` if the latest value can be updated.
    /// `Err()` if no receiver of this channel exists.
    ///
    /// # Examples
    /// ```
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// // Update the latest value.
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.recv());
    ///
    /// // Cannot update the value because the receiver does not exist.
    /// drop(receiver);
    /// assert!(updater.update(10).is_err());
    /// ```
    pub fn update(&self, value: T) -> Result<(), UpdateError<T>> {
        match self.destination.upgrade() {
            Some(dest) => {
                // lock() fails if another owner of the `Mutex` has panicked during holding the mutex.
                // But the owners (`Receiver` and other `Updater`) never panic at the time.
                // Therefore, this unwrap() always succeeds.
                dest.lock().unwrap().replace(value);
                Ok(())
            }
            None => Err(UpdateError(value)),
        }
    }
}

/// An error returned from the `update` function on a `Updater`.
///
/// This error occurs if the receiver has become disconnected, so the data could not be sent.
/// The data is returned back to the callee in this case.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct UpdateError<T>(T);

impl<T> Display for UpdateError<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The receiver has disconnected, so the data could not be sent."
        )
    }
}

impl<T: Debug> Error for UpdateError<T> {}

/// Creates a new channel, returning the updater/receiver halves.
///
/// The `Updater` can be cloned to send to the same channel multiple times, but only one `Receiver` is supported.
/// # Examples
/// ```
/// use single_buffer_channel::channel;
///
/// let (updater, receiver) = channel();
///
/// // Update the latest value.
/// updater.update(1).unwrap();
/// assert_eq!(Ok(1), receiver.recv());
/// ```
pub fn channel<T>() -> (Updater<T>, Receiver<T>) {
    let latest = Arc::from(Mutex::from(None));
    let weak = Arc::downgrade(&latest);
    let receiver = Receiver { latest };
    let updater = Updater { destination: weak };

    (updater, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::{sleep, spawn};

    #[test]
    fn test_update_recv() {
        let (updater, receiver) = channel();
        // Update
        updater.update(1).unwrap();
        //
        assert_eq!(Ok(1), receiver.recv());
        // Update
        updater.update(2).unwrap();
        //
        assert_eq!(Ok(2), receiver.recv());
    }

    #[test]
    fn test_multiple_update_recv() {
        let (updater, receiver) = channel();
        // Update
        updater.update(1).unwrap();
        updater.update(2).unwrap();
        // Receiver should receive only the latest value
        assert_eq!(Ok(2), receiver.recv());
    }

    #[test]
    fn test_multiple_updaters_recv() {
        let (updater, receiver) = channel();
        let updater2 = updater.clone();
        // Update
        updater.update(1).unwrap();
        assert_eq!(Ok(1), receiver.recv());
        updater2.update(2).unwrap();
        assert_eq!(Ok(2), receiver.recv());

        // Receiver should receive only the latest value
        updater.update(10).unwrap();
        updater2.update(20).unwrap();
        assert_eq!(Ok(20), receiver.recv());

        // channel is valid since at least 1 updater exists.
        drop(updater);
        updater2.update(200).unwrap();
        assert_eq!(Ok(200), receiver.recv());
    }

    #[test]
    fn test_recv_no_updater() {
        let (updater, receiver) = channel::<i32>();
        drop(updater);
        assert_eq!(Err(RecvError), receiver.recv());
    }

    #[test]
    fn test_try_recv() {
        let (updater, receiver) = channel();
        // Nothing updated yet
        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
        // Update
        updater.update(1).unwrap();
        // Receive
        assert_eq!(Ok(1), receiver.try_recv());
        // Nothing updated from the last reception
        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
    }

    #[test]
    fn test_try_recv_multiple_updaters() {
        let (updater, receiver) = channel();
        let updater2 = updater.clone();

        // Receiver should receive only the latest value
        updater.update(1).unwrap();
        updater2.update(2).unwrap();
        assert_eq!(Ok(2), receiver.try_recv());

        // Nothing updated from the last reception
        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
    }

    #[test]
    fn test_try_recv_no_updater() {
        let (updater, receiver) = channel::<i32>();
        drop(updater);
        assert_eq!(Err(TryRecvError::Disconnected), receiver.try_recv());
    }

    #[test]
    fn test_recv_timeout_ok() {
        let (updater, receiver) = channel::<i32>();
        let join_handle = spawn(move || {
            sleep(Duration::from_secs(1));
            updater.update(1).unwrap();
        });

        assert_eq!(Ok(1), receiver.recv_timeout(Duration::from_secs(2)));
        join_handle.join().unwrap();
    }

    #[test]
    fn test_recv_timeout_no_updater() {
        let (updater, receiver) = channel::<i32>();
        let join_handle = spawn(move || {
            drop(updater);
        });

        sleep(Duration::from_secs(1));

        assert_eq!(
            Err(RecvTimeoutError::Disconnected),
            receiver.recv_timeout(Duration::from_secs(1))
        );
        join_handle.join().unwrap();
    }

    #[test]
    fn test_recv_timeout_timeout() {
        let (updater, receiver) = channel::<i32>();
        let join_handle = spawn(move || {
            sleep(Duration::from_secs(2));
            drop(updater);
        });

        assert_eq!(
            Err(RecvTimeoutError::Timeout),
            receiver.recv_timeout(Duration::from_secs(1))
        );
        join_handle.join().unwrap();
    }

    #[test]
    fn test_update_no_receiver() {
        let (updater, receiver) = channel();
        drop(receiver);

        assert_eq!(Err(UpdateError(1)), updater.update(1));
    }
}
