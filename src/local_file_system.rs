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
    path::PathBuf,
};

use qubit_fs::{
    FileKind,
    FileMetadata,
    FileSystem,
    FileSystemCapabilities,
    FileSystemCapability,
    FileSystemId,
    FileSystemInfo,
    FileSystemProperties,
    FsError,
    FsErrorKind,
    FsOperation,
    FsPath,
    FsResult,
    NativePathCodec,
    OsStrPathCodec,
    PathSemantics,
};
use qubit_spi::ProviderId;

/// Provides the synchronous `file:` filesystem implementation.
///
/// This type has host-wide authority and accepts only native absolute paths.
/// It does not provide rooted sandbox semantics.
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
    ///
    /// A filesystem whose paths use hierarchical local semantics.
    ///
    /// # Panics
    ///
    /// Panics only if a static filesystem id, provider id, or URI scheme
    /// violates its corresponding grammar.
    #[must_use]
    pub fn host() -> Self {
        let id = FileSystemId::new("local-host")
            .expect("static local filesystem id must be valid");
        let provider_id = Self::provider_id();
        let info =
            FileSystemInfo::new(id, provider_id, PathSemantics::Hierarchical)
                .with_scheme("file")
                .expect("static file URI scheme must be valid");
        Self {
            info,
            capabilities: FileSystemCapabilities::default()
                .with(FileSystemCapability::Stat),
        }
    }

    /// Returns the identifier shared by the filesystem and its provider.
    ///
    /// # Returns
    ///
    /// The validated static `local-file` provider identifier.
    ///
    /// # Panics
    ///
    /// Panics only if the static provider identifier violates the provider-id
    /// grammar.
    pub(crate) fn provider_id() -> ProviderId {
        ProviderId::new("local-file")
            .expect("static local provider id must be valid")
    }

    /// Converts a canonical filesystem path into its native representation.
    ///
    /// # Parameters
    ///
    /// * `operation` - Public filesystem operation that needs the path.
    /// * `path` - Canonical filesystem path to convert.
    ///
    /// # Returns
    ///
    /// The losslessly reconstructed native path.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error when the decoded native path is relative.
    fn native_path(operation: FsOperation, path: &FsPath) -> FsResult<PathBuf> {
        let native_path = OsStrPathCodec
            .encode(path.as_str())
            .map(|path| PathBuf::from(path.into_owned()))
            .expect("validated FsPath text must encode as a native path");
        if !native_path.is_absolute() {
            return Err(FsError::new(
                FsErrorKind::InvalidPath,
                operation,
                "local filesystem path must be absolute",
            )
            .with_path(path.clone())
            .with_provider(Self::provider_id()));
        }
        Ok(native_path)
    }

    /// Converts one native metadata result into provider-neutral metadata.
    ///
    /// # Parameters
    ///
    /// * `metadata` - Native metadata captured without following a final link.
    ///
    /// # Returns
    ///
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
    ///
    /// A provider-neutral error with a scrubbed message and native source.
    fn map_io_error(
        operation: FsOperation,
        path: &FsPath,
        error: io::Error,
    ) -> FsError {
        FsError::from_io(error, operation)
            .with_path(path.clone())
            .with_provider(Self::provider_id())
    }
}

impl FileSystemProperties for LocalFileSystem {
    /// Returns immutable local provider information without performing I/O.
    #[inline]
    fn info(&self) -> &FileSystemInfo {
        &self.info
    }

    /// Returns immutable local capability guarantees without performing I/O.
    #[inline]
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
    ///
    /// Metadata captured without following the final symbolic link.
    ///
    /// # Errors
    ///
    /// Returns a path-aware filesystem error when native metadata lookup fails.
    fn stat(&self, path: &FsPath) -> FsResult<FileMetadata> {
        let native_path = Self::native_path(FsOperation::Stat, path)?;
        fs::symlink_metadata(native_path)
            .map(Self::map_metadata)
            .map_err(|error| Self::map_io_error(FsOperation::Stat, path, error))
    }
}
