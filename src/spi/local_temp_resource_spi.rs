// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Temporary-resource lifecycle adapter retaining native authority.

use std::path::PathBuf;

use qubit_fs::spi::{PersistRequest, SpiPersistFailure, TempResourceSpi};
use qubit_fs::{
    AchievedAtomicity, FsError, FsErrorKind, FsOperation, FsResult, PersistFailureState,
    PersistOutcome, PublicationMethod,
};
use qubit_local_files as native_files;

use crate::path::local_path_mapper;
use crate::spi::error_mapper;

/// Adapts one native temporary resource while retaining its authority mode.
#[must_use]
pub(crate) enum LocalTempResourceSpi {
    /// Owns a native temporary file until it reaches a terminal lifecycle
    /// state.
    File {
        /// Native resource retained for retry or cleanup while it is owned.
        resource: Option<native_files::LocalTempFile>,
        /// Whether paths must be translated relative to a rooted authority.
        rooted: bool,
    },
    /// Owns a native temporary directory until it reaches a terminal lifecycle
    /// state.
    Directory {
        /// Native resource retained for retry or cleanup while it is owned.
        resource: Option<native_files::LocalTempDirectory>,
        /// Whether paths must be translated relative to a rooted authority.
        rooted: bool,
    },
}
impl LocalTempResourceSpi {
    /// Wraps a host- or rooted-authority native temporary file.
    ///
    /// # Parameters
    ///
    /// - `value`: Native temporary file owned by the adapter.
    /// - `rooted`: Whether the native path belongs to a rooted authority.
    ///
    /// # Returns
    ///
    /// An active temporary-file lifecycle adapter.
    #[inline(always)]
    pub(crate) const fn file(value: native_files::LocalTempFile, rooted: bool) -> Self {
        Self::File {
            resource: Some(value),
            rooted,
        }
    }

    /// Wraps a host- or rooted-authority native temporary directory.
    ///
    /// # Parameters
    ///
    /// - `value`: Native temporary directory owned by the adapter.
    /// - `rooted`: Whether the native path belongs to a rooted authority.
    ///
    /// # Returns
    ///
    /// An active temporary-directory lifecycle adapter.
    #[inline(always)]
    pub(crate) const fn directory(value: native_files::LocalTempDirectory, rooted: bool) -> Self {
        Self::Directory {
            resource: Some(value),
            rooted,
        }
    }

    /// Maps a logical publication target through this resource's authority.
    ///
    /// # Parameters
    ///
    /// - `target`: Absolute logical publication target.
    ///
    /// # Returns
    ///
    /// The authority-local native target and whether the authority is rooted.
    ///
    /// # Errors
    ///
    /// Returns `InvalidPath` when the logical target cannot be converted to
    /// the resource's native authority.
    fn target(&self, target: &qubit_fs::Path) -> Result<(PathBuf, bool), SpiPersistFailure> {
        let rooted = match self {
            Self::File { rooted, .. } | Self::Directory { rooted, .. } => *rooted,
        };
        let target = if rooted {
            local_path_mapper::rooted(target)
        } else {
            local_path_mapper::host(target)
        }
        .map_err(persist_path_error)?;
        Ok((target, rooted))
    }
}

impl TempResourceSpi for LocalTempResourceSpi {
    /// Persists the native resource with the caller's replacement policy.
    ///
    /// # Parameters
    ///
    /// - `request`: Logical target and overwrite policy for publication.
    ///
    /// # Returns
    ///
    /// The published logical path, achieved atomicity, and publication method.
    ///
    /// # Errors
    ///
    /// Returns `InvalidState` when the resource is terminal, `InvalidPath`
    /// when the target cannot be mapped, a mapped I/O failure with native
    /// recovery state when publication fails, or an indeterminate failure when
    /// the published native path cannot be converted back to logical form.
    fn persist(
        &mut self,
        request: PersistRequest<'_>,
    ) -> Result<PersistOutcome, SpiPersistFailure> {
        let (target, rooted) = self.target(request.target())?;
        let options = persist_options(request.options().overwrite);
        let result = match self {
            Self::File { resource: slot, .. } => persist_file(slot, &target, options),
            Self::Directory { resource: slot, .. } => persist_directory(slot, &target, options),
        }?;
        map_persist_outcome(result, rooted)
    }
    /// Releases cleanup ownership without publishing the native resource.
    ///
    /// # Returns
    ///
    /// `Ok(())` after ownership is released.
    ///
    /// # Errors
    ///
    /// Returns `InvalidState` when the resource has already reached a terminal
    /// lifecycle state.
    fn keep(&mut self) -> FsResult<()> {
        match self {
            Self::File {
                resource: value, ..
            } => {
                let _ = value
                    .take()
                    .ok_or_else(|| {
                        FsError::new(
                            FsErrorKind::InvalidState,
                            FsOperation::KeepTemp,
                            "temporary resource is terminal",
                        )
                    })?
                    .keep();
                Ok(())
            }
            Self::Directory {
                resource: value, ..
            } => {
                let _ = value
                    .take()
                    .ok_or_else(|| {
                        FsError::new(
                            FsErrorKind::InvalidState,
                            FsOperation::KeepTemp,
                            "temporary resource is terminal",
                        )
                    })?
                    .keep();
                Ok(())
            }
        }
    }
    /// Removes the owned native resource through its creating authority.
    ///
    /// # Returns
    ///
    /// `Ok(())` after native cleanup completes.
    ///
    /// # Errors
    ///
    /// Returns `InvalidState` when the resource is terminal or a mapped I/O
    /// failure when native cleanup does not complete. Failed cleanup retains
    /// the native resource for retry.
    fn cleanup(&mut self) -> FsResult<()> {
        match self {
            Self::File {
                resource: value, ..
            } => value
                .as_mut()
                .ok_or_else(|| {
                    FsError::new(
                        FsErrorKind::InvalidState,
                        FsOperation::CleanupTemp,
                        "temporary resource is terminal",
                    )
                })?
                .cleanup()
                .map_err(file_cleanup_error),
            Self::Directory {
                resource: value, ..
            } => value
                .as_mut()
                .ok_or_else(|| {
                    FsError::new(
                        FsErrorKind::InvalidState,
                        FsOperation::CleanupTemp,
                        "temporary resource is terminal",
                    )
                })?
                .cleanup()
                .map_err(directory_cleanup_error),
        }
    }
}

/// Builds native temporary-resource persistence options.
///
/// # Parameters
///
/// - `overwrite`: Whether an existing destination may be replaced.
///
/// # Returns
///
/// Native persistence options with the requested replacement policy.
#[inline(always)]
fn persist_options(overwrite: bool) -> native_files::LocalPersistOptions {
    if overwrite {
        native_files::LocalPersistOptions::new().with_overwrite()
    } else {
        native_files::LocalPersistOptions::new()
    }
}

/// Maps a completed native persistence outcome to the facade representation.
///
/// # Parameters
///
/// - `result`: Completed native persistence outcome.
/// - `rooted`: Whether the published native path is authority-relative.
///
/// # Returns
///
/// The published logical path and achieved publication guarantees.
///
/// # Errors
///
/// Returns an indeterminate persistence failure when the published native path
/// cannot be converted back to canonical logical form.
fn map_persist_outcome(
    result: native_files::LocalPersistOutcome,
    rooted: bool,
) -> Result<PersistOutcome, SpiPersistFailure> {
    let logical = if rooted {
        local_path_mapper::rooted_logical(result.path())
    } else {
        local_path_mapper::host_logical(result.path())
    }
    .map_err(logical_persist_error)?;
    Ok(PersistOutcome::new(
        logical,
        if result.atomic() {
            AchievedAtomicity::Atomic
        } else {
            AchievedAtomicity::NonAtomic
        },
        match result.method() {
            native_files::LocalPersistMethod::AtomicRename => PublicationMethod::AtomicRename,
            _ => PublicationMethod::Direct,
        },
    ))
}

/// Persists a retained native temporary file and restores it after failure.
///
/// # Parameters
///
/// - `slot`: Adapter storage for the active native temporary file.
/// - `target`: Authority-local native publication target.
/// - `options`: Native overwrite policy.
///
/// # Returns
///
/// The completed native persistence outcome.
///
/// # Errors
///
/// Returns `InvalidState` when `slot` is empty. Native failures are mapped
/// with their recovery state and restore the returned resource into `slot`.
fn persist_file(
    slot: &mut Option<native_files::LocalTempFile>,
    target: &std::path::Path,
    options: native_files::LocalPersistOptions,
) -> Result<native_files::LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.persist_with_outcome(target, options) {
        Ok(result) => Ok(result),
        Err(error) => {
            let (io, resource, _, _, _, state) = error.into_parts_with_state();
            *slot = Some(resource);
            Err(SpiPersistFailure::new(
                error_mapper::map_io(
                    io,
                    FsOperation::PersistTemp,
                    "temporary file persistence failed",
                ),
                persist_failure_state(state),
            ))
        }
    }
}

/// Persists a retained native temporary directory and restores it after
/// failure.
///
/// # Parameters
///
/// - `slot`: Adapter storage for the active native temporary directory.
/// - `target`: Authority-local native publication target.
/// - `options`: Native overwrite policy.
///
/// # Returns
///
/// The completed native persistence outcome.
///
/// # Errors
///
/// Returns `InvalidState` when `slot` is empty. Native failures are mapped
/// with their recovery state and restore the returned resource into `slot`.
fn persist_directory(
    slot: &mut Option<native_files::LocalTempDirectory>,
    target: &std::path::Path,
    options: native_files::LocalPersistOptions,
) -> Result<native_files::LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.persist_with_outcome(target, options) {
        Ok(result) => Ok(result),
        Err(error) => {
            let (io, resource, _, _, _, state) = error.into_parts_with_state();
            *slot = Some(resource);
            Err(SpiPersistFailure::new(
                error_mapper::map_io(
                    io,
                    FsOperation::PersistTemp,
                    "temporary directory persistence failed",
                ),
                persist_failure_state(state),
            ))
        }
    }
}

/// Converts native persistence state to its portable equivalent.
///
/// # Parameters
///
/// - `state`: Native persistence failure state.
///
/// # Returns
///
/// The equivalent portable state; unknown future native states map to
/// `Indeterminate`.
#[inline]
fn persist_failure_state(state: native_files::LocalPersistFailureState) -> PersistFailureState {
    match state {
        native_files::LocalPersistFailureState::NotPublished => PersistFailureState::NotPublished,
        native_files::LocalPersistFailureState::PublishedSourceRetained => {
            PersistFailureState::PublishedSourceRetained
        }
        native_files::LocalPersistFailureState::Indeterminate => PersistFailureState::Indeterminate,
        _ => PersistFailureState::Indeterminate,
    }
}

/// Builds the failure returned for an already-terminal temporary resource.
///
/// # Returns
///
/// An `InvalidState` persistence failure with `NotPublished` state.
#[inline(always)]
fn terminal_persist_error() -> SpiPersistFailure {
    SpiPersistFailure::new(
        FsError::new(
            FsErrorKind::InvalidState,
            FsOperation::PersistTemp,
            "temporary resource is terminal",
        ),
        PersistFailureState::NotPublished,
    )
}

/// Wraps a target-path mapping error before native publication starts.
///
/// # Parameters
///
/// - `error`: Logical-to-native path conversion failure.
///
/// # Returns
///
/// A persistence failure with `NotPublished` state.
#[inline(always)]
fn persist_path_error(error: FsError) -> SpiPersistFailure {
    SpiPersistFailure::new(error, PersistFailureState::NotPublished)
}

/// Wraps a logical-path conversion error after native publication.
///
/// # Parameters
///
/// - `error`: Native-to-logical path conversion failure.
///
/// # Returns
///
/// A persistence failure with `Indeterminate` state because publication has
/// already completed.
#[inline(always)]
fn logical_persist_error(error: FsError) -> SpiPersistFailure {
    SpiPersistFailure::new(error, PersistFailureState::Indeterminate)
}

/// Maps temporary-resource cleanup I/O with local provider context.
///
/// # Parameters
///
/// - `error`: Native cleanup failure.
/// - `message`: Static context identifying the resource kind.
///
/// # Returns
///
/// A facade cleanup error.
#[inline(always)]
fn cleanup_error(error: std::io::Error, message: &'static str) -> FsError {
    error_mapper::map_io(error, FsOperation::CleanupTemp, message)
}

/// Maps a native temporary-file cleanup failure.
///
/// # Parameters
///
/// - `error`: Native cleanup failure.
///
/// # Returns
///
/// A facade cleanup error identifying temporary-file cleanup.
#[inline(always)]
fn file_cleanup_error(error: std::io::Error) -> FsError {
    cleanup_error(error, "temporary file cleanup failed")
}

/// Maps a native temporary-directory cleanup failure.
///
/// # Parameters
///
/// - `error`: Native cleanup failure.
///
/// # Returns
///
/// A facade cleanup error identifying temporary-directory cleanup.
#[inline(always)]
fn directory_cleanup_error(error: std::io::Error) -> FsError {
    cleanup_error(error, "temporary directory cleanup failed")
}
