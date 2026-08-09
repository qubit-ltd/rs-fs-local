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

use qubit_fs::CopyFailureState;
use qubit_fs::CopyStats;
use qubit_fs::FsError;
use qubit_fs::FsErrorKind;
use qubit_fs::FsOperation;
use qubit_fs::Path;
use qubit_fs::RenameFailureState;
use qubit_fs::spi::SpiCopyFailure;
use qubit_fs::spi::SpiRenameFailure;
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
    provider_id: &str,
) -> FsError {
    let kind = error_kind(&error);
    let error = FsError::with_source(kind, operation, "local filesystem operation failed", error)
        .with_path(path.clone())
        .with_provider(provider_id);
    match target {
        Some(target) => error.with_target(target.clone()),
        None => error,
    }
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
    provider_id: &str,
) -> FsError {
    let kind = error_kind(&error);
    FsError::with_source(kind, operation, message, error).with_provider(provider_id)
}

/// Wraps a path-conversion error before native copy starts.
///
/// # Parameters
///
/// - `error`: Logical-to-native path conversion failure.
///
/// # Returns
///
/// A copy failure with unchanged namespace state and empty statistics.
#[inline(always)]
pub(crate) fn copy_path_error(error: FsError) -> SpiCopyFailure {
    SpiCopyFailure::new(error, CopyFailureState::Unchanged, CopyStats::default())
}

/// Converts a complete native copy failure without discarding staging or
/// cleanup diagnostics retained by the native pipeline.
#[inline]
pub(crate) fn copy_failure(
    error: native_files::LocalCopyFailure,
    path: &Path,
    target: &Path,
    failure_path: Option<&Path>,
    failure_target: Option<&Path>,
    provider_id: &str,
) -> FsError {
    let kind = error_kind(error.error());
    let error = FsError::with_source(kind, FsOperation::Copy, "local copy failed", error)
        .with_path(path.clone())
        .with_target(target.clone())
        .with_provider(provider_id);
    let error = match failure_path {
        Some(failure_path) => error.with_failure_path(failure_path.clone()),
        None => error,
    };
    match failure_target {
        Some(failure_target) => error.with_failure_target(failure_target.clone()),
        None => error,
    }
}

/// Selects the most precise portable kind available from a native failure.
///
/// Native I/O sources are authoritative because the stable native category is
/// intentionally coarser than `std::io::ErrorKind`.
#[inline]
fn error_kind(error: &native_files::LocalFileError) -> FsErrorKind {
    match error.kind() {
        native_files::LocalFileErrorKind::InvalidPath => FsErrorKind::InvalidPath,
        native_files::LocalFileErrorKind::InvalidOptions => FsErrorKind::InvalidOptions,
        native_files::LocalFileErrorKind::InvalidState => FsErrorKind::InvalidState,
        native_files::LocalFileErrorKind::NotDirectory => FsErrorKind::NotDirectory,
        native_files::LocalFileErrorKind::IsDirectory => FsErrorKind::IsDirectory,
        native_files::LocalFileErrorKind::TypeConflict => FsErrorKind::Conflict,
        native_files::LocalFileErrorKind::Indeterminate => FsErrorKind::Indeterminate,
        native_files::LocalFileErrorKind::PublicationIncomplete => FsErrorKind::Io,
        _ if error.io_error().is_some() => io_kind(error.io_error_kind()),
        _ => native_kind(error.kind()),
    }
}

/// Wraps a path-conversion error before native rename starts.
///
/// # Parameters
///
/// - `error`: Logical-to-native path conversion failure.
///
/// # Returns
///
/// A rename failure with unchanged namespace state.
#[inline(always)]
pub(crate) fn rename_path_error(error: FsError) -> SpiRenameFailure {
    SpiRenameFailure::new(error, RenameFailureState::Unchanged)
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
        native_files::LocalFileErrorKind::InvalidPath => FsErrorKind::InvalidPath,
        native_files::LocalFileErrorKind::InvalidOptions => FsErrorKind::InvalidOptions,
        native_files::LocalFileErrorKind::InvalidState => FsErrorKind::InvalidState,
        native_files::LocalFileErrorKind::NotFound => FsErrorKind::NotFound,
        native_files::LocalFileErrorKind::AlreadyExists => FsErrorKind::AlreadyExists,
        native_files::LocalFileErrorKind::NotDirectory => FsErrorKind::NotDirectory,
        native_files::LocalFileErrorKind::IsDirectory => FsErrorKind::IsDirectory,
        native_files::LocalFileErrorKind::TypeConflict => FsErrorKind::Conflict,
        native_files::LocalFileErrorKind::PermissionDenied => FsErrorKind::PermissionDenied,
        native_files::LocalFileErrorKind::Unsupported => FsErrorKind::UnsupportedOperation,
        native_files::LocalFileErrorKind::RequirementNotMet => FsErrorKind::RequirementNotMet,
        native_files::LocalFileErrorKind::ResourceLimit => FsErrorKind::ResourceLimitExceeded,
        native_files::LocalFileErrorKind::DataCorruption => FsErrorKind::DataCorruption,
        native_files::LocalFileErrorKind::PublicationIncomplete => FsErrorKind::Io,
        native_files::LocalFileErrorKind::Indeterminate => FsErrorKind::Indeterminate,
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
        std::io::ErrorKind::NotADirectory => FsErrorKind::NotDirectory,
        std::io::ErrorKind::PermissionDenied => FsErrorKind::PermissionDenied,
        std::io::ErrorKind::InvalidInput => FsErrorKind::InvalidPath,
        std::io::ErrorKind::InvalidData => FsErrorKind::DataCorruption,
        std::io::ErrorKind::Interrupted => FsErrorKind::Interrupted,
        std::io::ErrorKind::TimedOut => FsErrorKind::Timeout,
        std::io::ErrorKind::Unsupported => FsErrorKind::UnsupportedOperation,
        std::io::ErrorKind::OutOfMemory => FsErrorKind::ResourceLimitExceeded,
        std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded => {
            FsErrorKind::QuotaExceeded
        }
        _ => FsErrorKind::Io,
    }
}
