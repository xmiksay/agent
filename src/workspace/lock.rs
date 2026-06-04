use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::{Context, Result};
use fs2::FileExt;

/// Holds an exclusive `flock` for as long as it is alive.
pub struct AdvisoryFileLock {
    file: File,
}

impl AdvisoryFileLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("opening lock file {}", path.display()))?;
        file.lock_exclusive()
            .with_context(|| format!("locking {}", path.display()))?;
        Ok(Self { file })
    }
}

impl Drop for AdvisoryFileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}
