// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Native error translation with public request context.

use qubit_fs::{
    FsError,
    FsErrorKind,
    FsOperation,
    Path,
};
use qubit_local_files as native_files;

/// Maps a native failure with caller-visible source and optional target paths.
///
/// # Parameters
///
/// - `error`: Native local-files failure to preserve as the source.
/// - `operation`: Facade operation that failed.
/// - `path`: Primary logical path supplied by the caller.
/// - `target`: Optional target path for two-path operations.
///
/// # Returns
///
/// A facade error with translated kind, provider identity, and path context.
#[inline]
pub(crate) fn map(
    error: native_files::LocalFileError,
    operation: FsOperation,
    path: &Path,
    target: Option<&Path>,
) -> FsError {
    let kind = native_kind(error.kind());
    let error = FsError::with_source(
        kind,
        operation,
        "local filesystem operation failed",
        error,
    )
    .with_path(path.clone())
    .with_provider("local-file");
    match target {
        Some(target) => error.with_target(target.clone()),
        None => error,
    }
}

/// Maps an I/O failure that has no caller-visible logical path.
///
/// # Parameters
///
/// - `error`: Standard I/O failure to preserve as the source.
/// - `operation`: Facade operation that failed.
/// - `message`: Static context describing the failed local action.
///
/// # Returns
///
/// A facade error with translated kind and local provider identity.
#[inline]
pub(crate) fn map_io(
    error: std::io::Error,
    operation: FsOperation,
    message: &'static str,
) -> FsError {
    FsError::with_source(io_kind(error.kind()), operation, message, error)
        .with_provider("local-file")
}

/// Maps a native local-files failure without inventing a logical path.
///
/// # Parameters
///
/// - `error`: Native local-files failure to preserve as the source.
/// - `operation`: Facade operation that failed.
/// - `message`: Static context describing the failed local action.
///
/// # Returns
///
/// A facade error with translated kind and local provider identity.
#[inline]
pub(crate) fn map_without_path(
    error: native_files::LocalFileError,
    operation: FsOperation,
    message: &'static str,
) -> FsError {
    FsError::with_source(native_kind(error.kind()), operation, message, error)
        .with_provider("local-file")
}

/// Converts a native local-files error kind to its facade equivalent.
///
/// # Parameters
///
/// - `kind`: Native error category to translate.
///
/// # Returns
///
/// The closest portable filesystem error category; unknown native categories
/// map to `Other`.
fn native_kind(kind: native_files::LocalFileErrorKind) -> FsErrorKind {
    match kind {
        native_files::LocalFileErrorKind::InvalidInput => {
            FsErrorKind::InvalidPath
        }
        native_files::LocalFileErrorKind::NotFound => FsErrorKind::NotFound,
        native_files::LocalFileErrorKind::AlreadyExists => {
            FsErrorKind::AlreadyExists
        }
        native_files::LocalFileErrorKind::TypeConflict => FsErrorKind::Conflict,
        native_files::LocalFileErrorKind::PermissionDenied => {
            FsErrorKind::PermissionDenied
        }
        native_files::LocalFileErrorKind::Unsupported => {
            FsErrorKind::UnsupportedOperation
        }
        native_files::LocalFileErrorKind::RequirementNotMet => {
            FsErrorKind::RequirementNotMet
        }
        native_files::LocalFileErrorKind::ResourceLimit => {
            FsErrorKind::ResourceLimitExceeded
        }
        native_files::LocalFileErrorKind::PublicationIncomplete => {
            FsErrorKind::Io
        }
        native_files::LocalFileErrorKind::Indeterminate => {
            FsErrorKind::Indeterminate
        }
        native_files::LocalFileErrorKind::Io => FsErrorKind::Io,
        _ => FsErrorKind::Other,
    }
}

/// Converts a standard I/O error kind to its facade equivalent.
///
/// # Parameters
///
/// - `kind`: Standard I/O error category to translate.
///
/// # Returns
///
/// The closest portable filesystem error category.
fn io_kind(kind: std::io::ErrorKind) -> FsErrorKind {
    match kind {
        std::io::ErrorKind::NotFound => FsErrorKind::NotFound,
        std::io::ErrorKind::AlreadyExists => FsErrorKind::AlreadyExists,
        std::io::ErrorKind::PermissionDenied => FsErrorKind::PermissionDenied,
        std::io::ErrorKind::InvalidInput => FsErrorKind::InvalidPath,
        std::io::ErrorKind::InvalidData => FsErrorKind::DataCorruption,
        std::io::ErrorKind::Interrupted => FsErrorKind::Interrupted,
        std::io::ErrorKind::TimedOut => FsErrorKind::Timeout,
        std::io::ErrorKind::Unsupported => FsErrorKind::UnsupportedOperation,
        std::io::ErrorKind::OutOfMemory
        | std::io::ErrorKind::StorageFull
        | std::io::ErrorKind::QuotaExceeded => {
            FsErrorKind::ResourceLimitExceeded
        }
        _ => FsErrorKind::Io,
    }
}
