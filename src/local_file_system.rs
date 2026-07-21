// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Defines the synchronous local filesystem implementation.

use std::{
    fs,
    io,
    path::Path,
};

use qubit_fs::{
    FileKind,
    FileMetadata,
    FileSystem,
    FileSystemCapabilities,
    FileSystemId,
    FileSystemInfo,
    FileSystemProperties,
    FsError,
    FsErrorKind,
    FsOperation,
    FsPath,
    FsResult,
    PathSemantics,
};
use qubit_spi::ProviderId;

/// Provides the synchronous `file:` filesystem implementation.
///
/// This initial form owns host-wide local filesystem authority. Rooted
/// authority is added through the same type without changing this public path.
pub struct LocalFileSystem {
    /// Immutable provider information returned without I/O.
    info: FileSystemInfo,
    /// Immutable operation guarantees returned without I/O.
    capabilities: FileSystemCapabilities,
}

impl LocalFileSystem {
    /// Creates a filesystem with authority over host-native absolute paths.
    ///
    /// # Returns
    /// A filesystem whose paths use hierarchical local semantics.
    #[must_use]
    pub fn host() -> Self {
        let id = FileSystemId::new("local-host")
            .expect("static local filesystem id must be valid");
        let provider_id = ProviderId::new("local-file")
            .expect("static local provider id must be valid");
        let info = FileSystemInfo::new(
            id,
            provider_id,
            PathSemantics::Hierarchical,
        )
        .with_scheme("file")
        .expect("static file URI scheme must be valid");
        Self {
            info,
            capabilities: FileSystemCapabilities::default(),
        }
    }

    /// Converts one native metadata result into provider-neutral metadata.
    ///
    /// # Parameters
    ///
    /// * `metadata` - Native metadata captured without following a final link.
    ///
    /// # Returns
    /// The corresponding file kind, byte length, and available timestamps.
    fn map_metadata(metadata: fs::Metadata) -> FileMetadata {
        let file_type = metadata.file_type();
        let kind = if file_type.is_file() {
            FileKind::File
        } else if file_type.is_dir() {
            FileKind::Directory
        } else if file_type.is_symlink() {
            FileKind::Symlink
        } else {
            FileKind::Other("native-special".to_owned())
        };
        let mut result = FileMetadata::new(kind);
        result.len = Some(metadata.len());
        result.modified_at = metadata.modified().ok();
        result.created_at = metadata.created().ok();
        result.accessed_at = metadata.accessed().ok();
        result
    }

    /// Converts one local I/O failure into a path-aware filesystem error.
    ///
    /// # Parameters
    ///
    /// * `operation` - Public filesystem operation that failed.
    /// * `path` - Provider-local path involved in the operation.
    /// * `error` - Native I/O error to retain as an opaque source.
    ///
    /// # Returns
    /// A provider-neutral error with a scrubbed message and native source.
    fn map_io_error(
        operation: FsOperation,
        path: &FsPath,
        error: io::Error,
    ) -> FsError {
        let kind = match error.kind() {
            io::ErrorKind::NotFound => FsErrorKind::NotFound,
            io::ErrorKind::AlreadyExists => FsErrorKind::AlreadyExists,
            io::ErrorKind::PermissionDenied => FsErrorKind::PermissionDenied,
            io::ErrorKind::NotADirectory => FsErrorKind::NotDirectory,
            io::ErrorKind::IsADirectory => FsErrorKind::IsDirectory,
            io::ErrorKind::InvalidInput => FsErrorKind::InvalidPath,
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => {
                FsErrorKind::QuotaExceeded
            }
            _ => FsErrorKind::Io,
        };
        FsError::with_source(kind, operation, "local filesystem operation failed", error)
            .with_path(path.clone())
    }
}

impl FileSystemProperties for LocalFileSystem {
    /// Returns immutable local provider information without performing I/O.
    fn info(&self) -> &FileSystemInfo {
        &self.info
    }

    /// Returns immutable local capability guarantees without performing I/O.
    fn capabilities(&self) -> FileSystemCapabilities {
        self.capabilities
    }
}

impl FileSystem for LocalFileSystem {
    /// Reads native metadata for one host-wide local path.
    ///
    /// # Parameters
    ///
    /// * `path` - Absolute provider-local path to inspect.
    ///
    /// # Returns
    /// Metadata captured without following the final symbolic link.
    ///
    /// # Errors
    /// Returns a path-aware filesystem error when native metadata lookup fails.
    fn stat(&self, path: &FsPath) -> FsResult<FileMetadata> {
        fs::symlink_metadata(Path::new(path.as_str()))
            .map(Self::map_metadata)
            .map_err(|error| Self::map_io_error(FsOperation::Stat, path, error))
    }
}
