//! Smart pointer to [`tokio::sync::RwLock`].

use std::time::Duration;

use crate::sync::{Error, Result, DEFAULT_TIMEOUT_DURATION};
use tokio::time::timeout;

/// Smart pointer to [`tokio::sync::RwLock`].
///
/// Wraps acquiring the lock into [`timeout`] with a [`Duration`] of 30 seconds
/// by default.
#[derive(Debug)]
pub struct RwLock<T> {
    /// The actual [`tokio::sync::Mutex`]
    inner: tokio::sync::RwLock<T>,
    /// The timeout duration
    timeout: Duration,
}

impl<T> RwLock<T> {
    /// Create new `RwLock` with default timeout of 30 seconds.
    pub fn new(value: T) -> Self {
        Self {
            inner: tokio::sync::RwLock::new(value),
            timeout: DEFAULT_TIMEOUT_DURATION,
        }
    }

    /// Create new `RwLock` with given timeout.
    pub fn new_with_timeout(value: T, timeout: Duration) -> Self {
        Self {
            inner: tokio::sync::RwLock::new(value),
            timeout,
        }
    }

    /// Wrapper around [`tokio::sync::RwLock::read()`]. Will time out if the
    /// lock can’t get acquired until the timeout is reached.
    ///
    /// # Panics
    ///
    /// Panics when timeout is reached.
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, T> {
        let read_guard = match timeout(self.timeout, self.inner.read()).await {
            Ok(read_guard) => read_guard,
            Err(_) => panic!("Timed out while waiting for `read` lock after {} ms.", self.timeout.as_millis()),
        };

        read_guard
    }

    /// Wrapper around [`tokio::sync::RwLock::read()`]. Will time out if the
    /// lock can't get acquired until the timeout is reached.
    ///
    /// Returns an error if timeout is reached.
    pub async fn read_err(&self) -> Result<tokio::sync::RwLockReadGuard<'_, T>> {
        let read_guard = timeout(self.timeout, self.inner.read())
            .await
            .map_err(|_| Error::ReadLockTimeout(self.timeout.as_millis()))?;

        Ok(read_guard)
    }

    /// Wrapper around [`tokio::sync::RwLock::write()`]. Will time out if
    /// the lock can't get acquired until the timeout is reached.
    ///
    ///  # Panics
    ///
    /// Panics when timeout is reached.
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, T> {
        let write_guard = match timeout(self.timeout, self.inner.write()).await {
            Ok(write_guard) => write_guard,
            Err(_) => panic!("Timed out while waiting for `write` lock after {} seconds.", self.timeout.as_secs()),
        };

        write_guard
    }

    /// Wrapper around [`tokio::sync::RwLock::write()`]. Will time out if
    /// the lock can't get acquired until the timeout is reached.
    ///
    /// Returns an error if timeout is reached.
    pub async fn write_err(&self) -> Result<tokio::sync::RwLockWriteGuard<'_, T>> {
        let write_guard = timeout(self.timeout, self.inner.write())
            .await
            .map_err(|_| Error::WriteLockTimeout(self.timeout.as_millis()))?;

        Ok(write_guard)
    }
}

impl<T> std::ops::Deref for RwLock<T> {
    type Target = tokio::sync::RwLock<T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> From<T> for RwLock<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}
