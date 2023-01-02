mod rwlock;

use std::time::Duration;

pub use rwlock::RwLock;
pub const DEFAULT_TIMEOUT_DURATION: Duration = Duration::from_millis(100);
pub type Result<T> = std::result::Result<T, Error>;

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        /// Mutex lock timeout error.
        LockTimeout(milliseconds: u128) {
            display("Timed out while waiting for `lock` after {0} ms.", milliseconds)
        }
        /// RwLock::read lock timeout error.
        ReadLockTimeout(milliseconds: u128) {
            display("Timed out while waiting for `read` lock after {0} ms.", milliseconds)
        }
        /// RwLock::write lock timeout error.
        WriteLockTimeout(milliseconds: u128) {
            display("Timed out while waiting for `write` lock after {0} ms..", milliseconds)
        }
        /// `tokio::sync::TryLockError` error.
        TokioSyncTryLock(err: tokio::sync::TryLockError) {
            from()
        }
    }
}
