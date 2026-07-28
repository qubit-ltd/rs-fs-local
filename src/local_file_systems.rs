//! Concrete facade factories for local filesystem adapters.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use qubit_fs::{FileSystem, FileSystemId, FsResult};

use crate::spi::{LocalFileSystemSpi, RootedLocalFileSystemSpi};

static ROOTED_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Factory for concrete host and rooted local filesystem facades.
pub enum LocalFileSystems {}

impl LocalFileSystems {
    /// Creates the process host filesystem facade.
    pub fn host() -> FsResult<FileSystem> {
        FileSystem::from_spi(LocalFileSystemSpi::new())
    }
    /// Opens `root` as a descriptor-backed rooted filesystem with a process-local identity.
    pub fn rooted(root: &Path) -> FsResult<FileSystem> {
        let id = {
            let value = format!(
                "local-rooted-{}-{}",
                std::process::id(),
                ROOTED_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            FileSystemId::new(&value)
        }?;
        Self::rooted_with_id(id, root)
    }
    /// Opens `root` with caller-specified stable identity.
    pub fn rooted_with_id(id: FileSystemId, root: &Path) -> FsResult<FileSystem> {
        FileSystem::from_spi(RootedLocalFileSystemSpi::open(id, root)?)
    }
}
