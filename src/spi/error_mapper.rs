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

use qubit_fs::spi::{
    SpiCopyFailure,
    SpiRenameFailure,
};
use qubit_fs::{
    CopyFailureState,
    CopyStats,
    FsError,
    FsErrorKind,
    FsOperation,
    Path,
    RenameFailureState,
};
use qubit_local_files as native_files;

use super::local_outcome_mapper;

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
    let kind = mapped_kind(&error);
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
    let kind = mapped_kind(&error);
    FsError::with_source(kind, operation, message, error)
        .with_provider("local-file")
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
    SpiCopyFailure::new(
        error,
        CopyFailureState::Unchanged,
        CopyStats::default(),
    )
}

/// Maps a native copy failure while retaining cleanup diagnostics as its
/// source.
///
/// # Parameters
///
/// - `error`: Native copy failure containing the primary error, state,
///   statistics, and optional staging cleanup diagnostics.
/// - `operation`: Facade operation that failed.
/// - `path`: Logical source path supplied by the caller.
/// - `target`: Logical destination path supplied by the caller.
///
/// # Returns
///
/// A copy failure whose public error retains the complete native failure as a
/// typed source, including any staging path and cleanup error.
#[inline]
pub(crate) fn map_copy_failure(
    error: native_files::LocalCopyFailure,
    operation: FsOperation,
    path: &Path,
    target: &Path,
) -> SpiCopyFailure {
    let state = local_outcome_mapper::copy_failure_state(error.state());
    let stats = error.partial_stats();
    let copy_stats = CopyStats {
        files: stats.files(),
        directories: stats.directories(),
        bytes: stats.bytes(),
        skipped: stats.skipped(),
        overwritten: stats.overwritten(),
        ..Default::default()
    };
    let kind = mapped_kind(error.error());
    let mapped = FsError::with_source(
        kind,
        operation,
        "local filesystem copy failed",
        error,
    )
    .with_path(path.clone())
    .with_target(target.clone())
    .with_provider("local-file");
    SpiCopyFailure::new(mapped, state, copy_stats)
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

/// Maps a native structured error using its retained I/O source when that
/// source provides a more precise portable category.
///
/// # Parameters
///
/// - `error`: Native structured error to classify.
///
/// # Returns
///
/// The most precise portable category supported by the native kind and source.
#[inline]
fn mapped_kind(error: &native_files::LocalFileError) -> FsErrorKind {
    match error.source_kind() {
        Some(native_files::LocalFileErrorSource::Io(source)) => {
            let mapped = io_kind(source.kind());
            if mapped == FsErrorKind::Io {
                native_kind(error.kind())
            } else {
                mapped
            }
        }
        Some(native_files::LocalFileErrorSource::PathCodec(_)) => {
            FsErrorKind::InvalidPath
        }
        None => native_kind(error.kind()),
        Some(_) => native_kind(error.kind()),
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
        std::io::ErrorKind::DirectoryNotEmpty => FsErrorKind::Conflict,
        std::io::ErrorKind::NotADirectory => FsErrorKind::NotDirectory,
        std::io::ErrorKind::IsADirectory => FsErrorKind::IsDirectory,
        std::io::ErrorKind::PermissionDenied => FsErrorKind::PermissionDenied,
        std::io::ErrorKind::InvalidInput => FsErrorKind::InvalidOptions,
        std::io::ErrorKind::InvalidData => FsErrorKind::DataCorruption,
        std::io::ErrorKind::Interrupted => FsErrorKind::Interrupted,
        std::io::ErrorKind::TimedOut => FsErrorKind::Timeout,
        std::io::ErrorKind::Unsupported => FsErrorKind::UnsupportedOperation,
        std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded => {
            FsErrorKind::QuotaExceeded
        }
        _ => FsErrorKind::Io,
    }
}
