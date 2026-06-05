//! Cross-platform single-instance process guard.
//!
//! On startup we try to acquire an exclusive, non-blocking lock on a known
//! file under the user's data dir. Holding the lock keeps a second invocation
//! from opening a duplicate window. The lock is automatically released by the
//! OS when the process exits — crash-recovery is free: the next launch
//! reclaims the lock on the leftover file without any cleanup ceremony.

use std::{
    fs::{File, OpenOptions},
    io,
    path::PathBuf,
};

use fs4::{FileExt, TryLockError};
use openlogi_core::paths::{self, PathsError};
use thiserror::Error;
use tracing::debug;

/// Held by `main` for the duration of the run; dropped on exit (the OS
/// releases the underlying file lock at the same time). The `_handle` field
/// is intentionally unused — the value is alive only for its `Drop` side
/// effect of closing the fd.
#[allow(
    dead_code,
    reason = "the File is held only so the OS keeps the lock — not read again"
)]
pub struct InstanceGuard {
    _handle: File,
}

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("could not resolve lock path")]
    Path(#[from] PathsError),
    #[error("could not open lock file at {path}")]
    Open {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("another OpenLogi instance already holds the lock at {path}")]
    AlreadyRunning { path: PathBuf },
    #[error("lock attempt at {path} failed")]
    LockFailed {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Acquire the single-instance lock. Returns `Ok(guard)` on success — keep
/// the guard alive until the process is about to exit.
///
/// `AlreadyRunning` is the polite "another copy is open" signal callers
/// surface to the user (and exit with a non-error status). Other variants
/// indicate filesystem trouble.
pub fn acquire() -> Result<InstanceGuard, InstanceError> {
    let path = paths::config_dir()?.join(crate::platform::branding::instance_lock_file());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| InstanceError::Open {
            path: path.clone(),
            source,
        })?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| InstanceError::Open {
            path: path.clone(),
            source,
        })?;
    match FileExt::try_lock(&file) {
        Ok(()) => {
            debug!(path = %path.display(), "single-instance lock acquired");
            Ok(InstanceGuard { _handle: file })
        }
        Err(TryLockError::WouldBlock) => Err(InstanceError::AlreadyRunning { path }),
        Err(TryLockError::Error(source)) => Err(InstanceError::LockFailed { path, source }),
    }
}
