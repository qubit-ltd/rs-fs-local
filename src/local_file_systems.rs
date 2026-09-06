// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Concrete facade factories for local filesystem adapters.

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use qubit_fs::FileSystem;
use qubit_fs::Path as LogicalPath;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_fs::metadata::FileSystemId;
use qubit_local_files as native_files;
#[cfg(feature = "registry")]
use qubit_spi::ProviderId;

use crate::LocalResourcePolicy;
use crate::spi::LocalFileSystemSpi;

/// Monotonic process-local suffix for generated rooted filesystem identities.
static ROOTED_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Factory for concrete host and rooted local filesystem facades.
pub struct LocalFileSystems {
    /// Prevents construction outside this crate while retaining a type
    /// namespace for public factory methods.
    _private: (),
}

impl LocalFileSystems {
    /// Creates the process host filesystem facade.
    ///
    /// # Returns
    ///
    /// A concrete synchronous filesystem using the process host namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the static host SPI cannot be assembled into a
    /// concrete filesystem.
    #[inline(always)]
    pub fn host(policy: LocalResourcePolicy) -> FsResult<FileSystem> {
        FileSystem::from_spi(LocalFileSystemSpi::new(policy)?)
    }

    /// Opens `root` as a descriptor-backed rooted filesystem with a
    /// process-local identity.
    ///
    /// This performs native filesystem I/O to retain `root`.
    ///
    /// # Parameters
    ///
    /// - `root`: Native directory to retain as filesystem authority.
    ///
    /// # Returns
    ///
    /// A rooted synchronous filesystem with a process-local unique identity.
    ///
    /// # Errors
    ///
    /// Returns an error when `root` cannot be opened or the rooted filesystem
    /// cannot be assembled.
    ///
    /// # Panics
    ///
    /// Panics only if the decimal process identifier and monotonic counter
    /// unexpectedly fail the static filesystem-identity syntax.
    pub fn rooted(
        root: &Path,
        policy: LocalResourcePolicy,
    ) -> FsResult<FileSystem> {
        let id = {
            let value = format!(
                "local-rooted-{}-{}",
                std::process::id(),
                ROOTED_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            FileSystemId::new(&value).expect(
                "process id and monotonic counter form a valid filesystem id",
            )
        };
        Self::rooted_with_id(id, root, policy)
    }

    /// Opens `root` with caller-specified stable identity.
    ///
    /// This performs native filesystem I/O to retain `root`.
    ///
    /// # Parameters
    ///
    /// - `id`: Stable identity exposed by the rooted filesystem.
    /// - `root`: Native directory to retain as filesystem authority.
    ///
    /// # Returns
    ///
    /// A rooted synchronous filesystem using the supplied identity.
    ///
    /// # Errors
    ///
    /// Returns an error when `root` cannot be opened or the rooted filesystem
    /// cannot be assembled with `id`.
    #[inline(always)]
    pub fn rooted_with_id(
        id: FileSystemId,
        root: &Path,
        policy: LocalResourcePolicy,
    ) -> FsResult<FileSystem> {
        FileSystem::from_spi(LocalFileSystemSpi::rooted(id, root, policy)?)
    }

    /// Opens `root` with a registry-validated provider identity.
    ///
    /// This is intended for applications that register multiple rooted local
    /// providers in one provider registry.
    #[cfg(feature = "registry")]
    pub(crate) fn rooted_with_provider_id(
        id: FileSystemId,
        provider_id: &ProviderId,
        root: &Path,
        policy: LocalResourcePolicy,
    ) -> FsResult<FileSystem> {
        FileSystem::from_spi(LocalFileSystemSpi::rooted_with_provider_id(
            id,
            provider_id.as_str(),
            root,
            policy,
        )?)
    }
}

/// Converts an absolute host-native path to a canonical logical path.
///
/// This conversion preserves platform-native path components, including
/// non-UTF-8 Unix names, without routing through lossy display text.
pub fn host_path_to_logical(path: &Path) -> FsResult<LogicalPath> {
    crate::path::local_path_mapper::logical(
        native_files::path::LocalFileSystemScope::Host,
        path,
        FsOperation::ParsePath,
    )
}
