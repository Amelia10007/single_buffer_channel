//! Multi-producer, single-consumer ***SINGLE*** data communication primitives.
//!
//! This crate provides a latest-message style [`channel`](fn.channel.html),
//! where [`Updater`](struct.Updater.html)(s) can update the latest data that the [`Receiver`](struct.Receiver.html) owns it.
//!
//! Unlike the `std::sync::mpsc::channel`, by using `channel` of this crate, each data send will overwrite the original data.
//! Once the `receiver` receives the data of its channel, `receiver` can retrieve nothing unless the updater(s) updates the data.
//! These property are useful, for example, when a thread is interested in the ***latest*** result of another continually working thread.
//!
//! # Examples
//! ## Basic usage
//! ```
//! use single_buffer_channel::channel;
//!
//! let (updater, receiver) = channel();
//!
//! std::thread::spawn(move || {
//!     updater.update(1).unwrap();
//! })
//! .join().unwrap();
//!
//! assert_eq!(Ok(1), receiver.recv());
//! ```
//! ## Multiple update
//! ```
//! use single_buffer_channel::channel;
//!
//! let (updater, receiver) = channel();
//! let updater2 = updater.clone(); // updater can be cloned.
//!
//! // Only the latest data will be received
//! updater.update(1).unwrap();
//! updater.update(2).unwrap();
//! assert_eq!(Ok(2), receiver.recv());
//!
//! updater.update(10).unwrap();
//! updater2.update(20).unwrap();
//! assert_eq!(Ok(20), receiver.recv());
//!
//! // Channel is valid as long as at least 1 updater exists.
//! drop(updater);
//! updater2.update(200).unwrap();
//! assert_eq!(Ok(200), receiver.recv());
//! ```
//! ## Receive after updater dropped
//! ```
//! use single_buffer_channel::channel;
//!
//! let (updater, receiver) = channel::<i32>();
//!
//! drop(updater);
//! // The updater dropped and no data exists on the buffer. So recv() fails.
//! assert!(receiver.recv().is_err());
//! ```
//! ## Receive after update and updater dropped
//! ```
//! use single_buffer_channel::channel;
//!
//! let (updater, receiver) = channel();
//!
//! updater.update(1).unwrap();
//! drop(updater);
//!
//! // The updater dropped. but the updated data remains, so recv() succeeds.
//! assert_eq!(Ok(1), receiver.recv());
//!
//! // The updater dropped and no data exists on the buffer. So recv() fails.
//! assert!(receiver.recv().is_err());
//! ```

use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::{Arc, Mutex, Weak};

/// The receiving-half of the single data channel.
///
/// Latest data of the channel can be retrieved using `recv` and `try_recv`.
#[derive(Debug)]
pub struct Receiver<T> {
    /// Stores the latest data as `Some(data)`, or `None` if updater(s) has not call its `update` yet.
    buffer: Arc<Mutex<Option<T>>>,
}

impl<T> Receiver<T> {
    /// Waits the latest data of this channel. This method blocks the current thread.
    ///
    /// After successful reception, nothing can be retrieved unless the updater(s) updates the data.
    /// # Returns
    /// `Ok()` if the latest data can be retrieved.
    /// `Err()` if the data has not been updated since the last call of `recv` or `try_recv` and there is no updater of this channel.
    ///
    /// # Examples
    /// ## Basic usage
    /// ```
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.recv());
    /// ```
    /// ## Receive after updater dropped
    /// ```
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel::<i32>();
    ///
    /// drop(updater);
    /// // The updater dropped and no data exists on the buffer. So recv() fails.
    /// assert!(receiver.recv().is_err());
    /// ```
    /// ## Receive after update and updater dropped
    /// ```
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// updater.update(1).unwrap();
    /// drop(updater);
    ///
    /// // The updater dropped. but the updated data remains, so recv() succeeds.
    /// assert_eq!(Ok(1), receiver.recv());
    ///
    /// // The updater dropped and no data exists on the buffer. So recv() fails.
    /// assert!(receiver.recv().is_err());
    /// ```
    pub fn recv(&self) -> Result<T, RecvError> {
        loop {
            match self.try_recv() {
                Ok(data) => break Ok(data),
                Err(TryRecvError::Disconnected) => break Err(RecvError),
                _ => {}
            }
        }
    }

    /// Attempts to receive the latest data of this channel. This method does not block the current thread.
    ///
    /// After successful reception, nothing can be retrieved unless the updater(s) updates the data.
    /// *See also the examples of [`Receiver::recv`](struct.Receiver.html#method.recv) to understand behaviors when the updater(s) has disconnected.*
    /// # Returns
    /// `Ok()` if the latest data can be retrieved.
    /// `Err()` if
    /// - The data has not been updated since the last call of `recv` or `try_recv`.
    /// - There is no updater of this channel.
    ///
    /// # Examples
    /// ```
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    /// // Nothing updated yet.
    /// assert!(receiver.try_recv().is_err());
    ///
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.try_recv());
    ///
    /// // Nothing updated yet from the last reception.
    /// assert!(receiver.try_recv().is_err());
    /// ```
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        // lock() fails if another owner of the mutex has panicked during holding it.
        // But the owners (updater) never panic at the time.
        // Therefore, this unwrap() always succeeds.
        match self.buffer.lock().unwrap().take() {
            Some(data) => Ok(data),
            None => {
                let has_updater = self.has_updater();
                if has_updater {
                    Err(TryRecvError::Empty)
                } else {
                    Err(TryRecvError::Disconnected)
                }
            }
        }
    }

    /// Returns `true` if there is at least 1 updater of this channel.
    fn has_updater(&self) -> bool {
        Arc::weak_count(&self.buffer) != 0
    }
}

/// An error returned from the `recv` function on a `Receiver`.
///
/// This error occurs if `Receiver` has become unable to receive data anymore due to the updater(s)' disconnection.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RecvError;

impl Display for RecvError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "This receiver has become unable to receive data anymore due to the updater(s)' disconnection.")
    }
}

impl Error for RecvError {}

/// An error returned from the `try_recv` function on a `Receiver`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TryRecvError {
    /// This error occurs if `Receiver` has become unable to receive data anymore due to the updater(s)' disconnection.
    Disconnected,
    /// This channel is currently empty, but the updater(s) have not yet disconnected, so data may yet become available.
    Empty,
}

impl Display for TryRecvError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let msg = match self {
            TryRecvError::Disconnected => "This receiver has become unable to receive data anymore due to the updater(s)' disconnection.",
            TryRecvError::Empty => "This channel is currently empty, but the updater(s) have not disconnected yet, so data may become available.",
        };
        write!(f, "{}", msg)
    }
}

impl Error for TryRecvError {}

/// The updating-half of the single data channel.
///
/// Data of this the channel can be updated by `update`.
/// `update` will overwrite the existing data of the channel.
///
/// The `Updater` can be cloned to update to the same channel multiple times.
#[derive(Debug)]
pub struct Updater<T> {
    /// Destination to the buffer of the recevier.
    dest: Weak<Mutex<Option<T>>>,
}

impl<T> Updater<T> {
    /// Updates the data of this channel.
    /// Previous data will be overwritten if it exists.
    /// # Returns
    /// `Ok()` if the data is updated.
    /// `Err()` if there is no receiver of this channel.
    ///
    /// # Examples
    /// ## Basic usage
    /// ```
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.recv());
    /// ```
    /// ## `update` after `Receiver` dropped
    /// ```
    /// use single_buffer_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// // update() fails because there is no receiver of this channel.
    /// drop(receiver);
    /// assert!(updater.update(1).is_err());
    /// ```
    pub fn update(&self, data: T) -> Result<(), UpdateError<T>> {
        match self.dest.upgrade() {
            Some(dest) => {
                // lock() fails if another owner of the mutex has panicked during holding it.
                // But the owners (receiver and other updaters) never panic at the time.
                // Therefore, this unwrap() always succeeds.
                dest.lock().unwrap().replace(data);
                Ok(())
            }
            None => Err(UpdateError(data)),
        }
    }
}

impl<T> Clone for Updater<T> {
    fn clone(&self) -> Self {
        Self {
            dest: self.dest.clone(),
        }
    }
}

/// An error returned from the `update` function on a `Updater`.
///
/// This error occurs if the receiver has become disconnected, so the data could not be updated.
/// The data is returned back to the callee in this case.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct UpdateError<T>(T);

impl<T> Display for UpdateError<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The receiver has disconnected, so the data could not be updated."
        )
    }
}

impl<T: Debug> Error for UpdateError<T> {}

/// Creates a new channel, returning the updater/receiver halves.
///
/// The `Updater` can be cloned to update to the same channel multiple times, but only one `Receiver` is supported.
/// # Examples
/// ```
/// use single_buffer_channel::channel;
///
/// let (updater, receiver) = channel();
///
/// updater.update(1).unwrap();
/// assert_eq!(Ok(1), receiver.recv());
/// ```
pub fn channel<T>() -> (Updater<T>, Receiver<T>) {
    let buffer = Arc::from(Mutex::from(None));
    let dest = Arc::downgrade(&buffer);
    let receiver = Receiver { buffer };
    let updater = Updater { dest };

    (updater, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_recv() {
        let (updater, receiver) = channel();

        updater.update(1).unwrap();
        assert_eq!(Ok(1), receiver.recv());

        updater.update(2).unwrap();
        assert_eq!(Ok(2), receiver.recv());
    }

    #[test]
    fn test_multiple_update_recv() {
        let (updater, receiver) = channel();
        // Update
        updater.update(1).unwrap();
        updater.update(2).unwrap();

        // Only the latest data will be received
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

        // Only the latest data will be received
        updater.update(10).unwrap();
        updater2.update(20).unwrap();
        assert_eq!(Ok(20), receiver.recv());

        // Channel is valid since at least 1 updater exists.
        drop(updater);
        updater2.update(200).unwrap();
        assert_eq!(Ok(200), receiver.recv());
    }

    #[test]
    fn test_update_recv_multithread() {
        let (updater, receiver) = channel();

        // Update on another thread
        std::thread::spawn(move || {
            updater.update(1).unwrap();
            updater.update(2).unwrap();

            // Updater dropped here, but there is data in buffer of the channel.
            // The data can be retrieved by the receiver.
        })
        .join()
        .unwrap();

        // Only the latest data will be received
        assert_eq!(Ok(2), receiver.recv());
    }

    #[test]
    fn test_recv_from_remaining_data_disconnected_updater() {
        let (updater, receiver) = channel();
        updater.update(1).unwrap();
        drop(updater);

        // The updater dropped.
        // but the updated data remains, so recv() succeeds.
        assert_eq!(Ok(1), receiver.recv());
    }

    #[test]
    fn test_recv_no_data_disconnected_updater() {
        let (updater, receiver) = channel::<i32>();
        drop(updater);

        // The updater dropped and no data exists on the buffer.
        // So recv() fails.
        assert_eq!(Err(RecvError), receiver.recv());
    }

    #[test]
    fn test_try_recv() {
        let (updater, receiver) = channel();

        // Nothing updated yet
        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
        // Update/receive
        updater.update(1).unwrap();
        assert_eq!(Ok(1), receiver.try_recv());

        // Nothing updated from the last reception
        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
    }

    #[test]
    fn test_try_recv_multiple_updaters() {
        let (updater, receiver) = channel();
        let updater2 = updater.clone();

        // Only the latest data will be received
        updater.update(1).unwrap();
        updater2.update(2).unwrap();
        assert_eq!(Ok(2), receiver.try_recv());

        // Nothing updated from the last reception
        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
    }

    #[test]
    fn test_try_recv_from_remaining_data_disconnected_updater() {
        let (updater, receiver) = channel();
        updater.update(1).unwrap();
        drop(updater);

        // The updater dropped.
        // but the updated data remains, so try_recv() succeeds.
        assert_eq!(Ok(1), receiver.try_recv());

        // try_recv fails because nothing updated from the last reception and the updater dropped.
        assert_eq!(Err(TryRecvError::Disconnected), receiver.try_recv());
    }

    #[test]
    fn test_try_recv_no_updater() {
        let (updater, receiver) = channel::<i32>();
        drop(updater);

        assert_eq!(Err(TryRecvError::Disconnected), receiver.try_recv());
    }

    #[test]
    fn test_update_no_receiver() {
        let (updater, receiver) = channel();
        drop(receiver);

        assert_eq!(Err(UpdateError(1)), updater.update(1));
    }
}
