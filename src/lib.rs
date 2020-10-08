use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Receiver<T> {
    latest: Arc<Mutex<Option<T>>>,
}

impl<T> Receiver<T> {
    pub fn recv(&self) -> Result<T, RecvError> {
        loop {
            match self.try_recv() {
                Ok(value) => break Ok(value),
                Err(TryRecvError::Disconnected) => break Err(RecvError),
                _ => {}
            }
        }
    }

    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        if !self.has_updater() {
            Err(TryRecvError::Disconnected)
        } else {
            // lock() fails if another owner of the `Mutex` has panicked during holding the mutex.
            // But the owners (`Updater`) nerver panic during the time.
            // Therefore, this unwrap() always succeeds.
            match self.latest.lock().unwrap().take() {
                Some(value) => Ok(value),
                None => Err(TryRecvError::Empty),
            }
        }
    }

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

    fn has_updater(&self) -> bool {
        Arc::weak_count(&self.latest) != 0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RecvError;

impl Display for RecvError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "The sender(s) has become disconnected, and there will never be any more data received on it.")
    }
}

impl Error for RecvError {}

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

#[derive(Debug, Clone)]
pub struct Updater<T> {
    destination: Weak<Mutex<Option<T>>>,
}

impl<T> Updater<T> {
    pub fn update(&self, value: T) -> Result<(), UpdateError<T>> {
        match self.destination.upgrade() {
            Some(dest) => {
                dest.lock().unwrap().replace(value);
                Ok(())
            }
            None => Err(UpdateError(value)),
        }
    }
}

/// The receiver has disconnected, so the data could not be sent. The data is returned back to the callee in this case.
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
