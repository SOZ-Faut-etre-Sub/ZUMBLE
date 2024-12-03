mod rwlock;

use std::time::Duration;
use thiserror::Error;

pub const DEFAULT_TIMEOUT_DURATION: Duration = Duration::from_millis(100);
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    /// RwLock::read lock timeout error.
    #[error("Timed out while waiting for `read` lock after {0} ms.")]
    ReadLockTimeout(u128),
    /// RwLock::write lock timeout error.
    #[error("Timed out while waiting for `write` lock after {0} ms.")]
    WriteLockTimeout(u128),
}
