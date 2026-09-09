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

use std::path::Path;
use std::path::PathBuf;

use qubit_fs::Path as LogicalPath;
use qubit_fs::error::FsEffectState;
use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_fs::metadata::AchievedAtomicity;
use qubit_fs::metadata::PublicationMethod;
use qubit_fs::spi::PersistRequest;
use qubit_fs::spi::SpiPersistFailure;
use qubit_fs::spi::TempResourceSpi;
use qubit_fs::temp::PersistCleanupState;
use qubit_fs::temp::PersistFailureState;
use qubit_fs::temp::PersistOptions;
use qubit_fs::temp::PersistOutcome;
use qubit_local_files as native_files;

use super::local_temp_path_projection::LocalTempPathProjection;
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
        /// Provider identity attached to lifecycle failures.
        provider_id: String,
        /// Requested parent spelling, independent of native cleanup authority.
        projection: Option<LocalTempPathProjection>,
    },
    /// Owns a native temporary directory until it reaches a terminal lifecycle
    /// state.
    Directory {
        /// Native resource retained for retry or cleanup while it is owned.
        resource: Option<native_files::LocalTempDirectory>,
        /// Whether paths must be translated relative to a rooted authority.
        rooted: bool,
        /// Provider identity attached to lifecycle failures.
        provider_id: String,
        /// Requested parent spelling, independent of native cleanup authority.
        projection: Option<LocalTempPathProjection>,
    },
}
impl LocalTempResourceSpi {
    /// Wraps a host- or rooted-authority native temporary file.
    ///
    /// # Parameters
    ///
    /// - `value`: Native temporary file owned by the adapter.
    /// - `rooted`: Whether the native path belongs to a rooted authority.
    /// - `provider_id`: Provider identity attached to lifecycle failures.
    /// - `projection`: Optional requested-parent spelling for generated paths.
    ///
    /// # Returns
    ///
    /// An active temporary-file lifecycle adapter.
    #[inline(always)]
    pub(crate) fn file(
        value: native_files::LocalTempFile,
        rooted: bool,
        provider_id: String,
        projection: Option<LocalTempPathProjection>,
    ) -> Self {
        Self::File {
            resource: Some(value),
            rooted,
            provider_id,
            projection,
        }
    }

    /// Wraps a host- or rooted-authority native temporary directory.
    ///
    /// # Parameters
    ///
    /// - `value`: Native temporary directory owned by the adapter.
    /// - `rooted`: Whether the native path belongs to a rooted authority.
    /// - `provider_id`: Provider identity attached to lifecycle failures.
    /// - `projection`: Optional requested-parent spelling for generated paths.
    ///
    /// # Returns
    ///
    /// An active temporary-directory lifecycle adapter.
    #[inline(always)]
    pub(crate) fn directory(
        value: native_files::LocalTempDirectory,
        rooted: bool,
        provider_id: String,
        projection: Option<LocalTempPathProjection>,
    ) -> Self {
        Self::Directory {
            resource: Some(value),
            rooted,
            provider_id,
            projection,
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
    fn target(&self, target: &LogicalPath) -> Result<(PathBuf, bool), SpiPersistFailure> {
        let rooted = match self {
            Self::File { rooted, .. } | Self::Directory { rooted, .. } => *rooted,
        };
        let target = local_path_mapper::native(
            if rooted {
                native_files::path::LocalFileSystemScope::Rooted
            } else {
                native_files::path::LocalFileSystemScope::Host
            },
            target,
        )
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
    /// recovery state when publication fails. Successful publication retains
    /// the requested logical target without another path conversion.
    fn persist(&mut self, request: PersistRequest<'_>) -> Result<PersistOutcome, SpiPersistFailure> {
        let (target, _) = self.target(request.target())?;
        let provider_id = match self {
            Self::File { provider_id, .. } | Self::Directory { provider_id, .. } => provider_id.to_owned(),
        };
        let options = persist_options(request.options());
        let result = match self {
            Self::File { resource: slot, .. } => persist_file(slot, &target, request.target(), options, &provider_id),
            Self::Directory { resource: slot, .. } => {
                persist_directory(slot, &target, request.target(), options, &provider_id)
            }
        }?;
        Ok(map_persist_outcome(result, request.target().clone()))
    }
    /// Publishes the native resource to its generated sibling target.
    ///
    /// # Returns
    ///
    /// The generated logical target and confirmed publication facts.
    ///
    /// # Errors
    ///
    /// Returns a recoverable provider failure when publication does not
    /// complete.
    fn keep(&mut self) -> Result<PersistOutcome, SpiPersistFailure> {
        let (rooted, provider_id) = match self {
            Self::File {
                rooted, provider_id, ..
            }
            | Self::Directory {
                rooted, provider_id, ..
            } => (*rooted, provider_id.to_owned()),
        };
        let result = match self {
            Self::File { resource, .. } => keep_file(resource, &provider_id),
            Self::Directory { resource, .. } => keep_directory(resource, &provider_id),
        }?;
        let projection = match self {
            Self::File { projection, .. } | Self::Directory { projection, .. } => projection.as_ref(),
        };
        let projected = projection
            .map(|projection| projection.project(result.path(), FsOperation::KeepTemp))
            .transpose()
            .map_err(logical_persist_error)?;
        let logical = local_path_mapper::logical(
            if rooted {
                native_files::path::LocalFileSystemScope::Rooted
            } else {
                native_files::path::LocalFileSystemScope::Host
            },
            projected.as_deref().unwrap_or(result.path()),
            FsOperation::KeepTemp,
        )
        .map_err(logical_persist_error)?;
        Ok(map_persist_outcome(result, logical))
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
                resource: value,
                provider_id,
                ..
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
                .map_err(|error| file_cleanup_error(error, provider_id)),
            Self::Directory {
                resource: value,
                provider_id,
                ..
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
                .map_err(|error| directory_cleanup_error(error, provider_id)),
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
fn persist_options(options: &PersistOptions) -> native_files::options::LocalPersistOptions {
    let mut native = native_files::options::LocalPersistOptions::new();
    if options.overwrite() {
        native = native.with_overwrite();
    }
    if options.creates_parent() {
        native = native.with_create_parent();
    }
    native
}

/// Maps a completed native persistence outcome to the facade representation.
///
/// # Parameters
///
/// - `result`: Completed native persistence outcome.
/// - `logical`: Caller-visible target in the portable namespace.
///
/// # Returns
///
/// The published logical path and achieved publication guarantees.
fn map_persist_outcome(result: native_files::outcome::LocalPersistOutcome, logical: LogicalPath) -> PersistOutcome {
    PersistOutcome::new(
        logical,
        if result.atomic() {
            AchievedAtomicity::Atomic
        } else {
            AchievedAtomicity::NonAtomic
        },
        match result.method() {
            native_files::outcome::LocalPersistMethod::AtomicRename => PublicationMethod::AtomicRename,
            _ => PublicationMethod::Direct,
        },
    )
    .with_cleanup_state(match result.cleanup_state() {
        native_files::outcome::LocalPersistCleanupState::Complete => PersistCleanupState::Complete,
        native_files::outcome::LocalPersistCleanupState::ResidualSandbox => {
            PersistCleanupState::ResidualTemporaryContainer
        }
        _ => PersistCleanupState::ResidualTemporaryContainer,
    })
}

/// Persists a retained native temporary file and restores it after failure.
///
/// # Parameters
///
/// - `slot`: Adapter storage for the active native temporary file.
/// - `target`: Authority-local native publication target.
/// - `logical_target`: Caller-visible publication target retained on failure.
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
    target: &Path,
    logical_target: &LogicalPath,
    options: native_files::options::LocalPersistOptions,
    provider_id: &str,
) -> Result<native_files::outcome::LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.persist_with(target, options) {
        Ok(result) => Ok(result),
        Err(error) => {
            let (error, resource, _, _, _, state) = error.into_parts_with_state();
            *slot = Some(resource);
            let state = persist_failure_state(state);
            Err(SpiPersistFailure::new(
                error_mapper::map(error, FsOperation::PersistTemp, logical_target, None, provider_id)
                    .with_effect_state(persist_effect_state(state)),
                state,
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
/// - `logical_target`: Caller-visible publication target retained on failure.
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
    target: &Path,
    logical_target: &LogicalPath,
    options: native_files::options::LocalPersistOptions,
    provider_id: &str,
) -> Result<native_files::outcome::LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.persist_with(target, options) {
        Ok(result) => Ok(result),
        Err(error) => {
            let (error, resource, _, _, _, state) = error.into_parts_with_state();
            *slot = Some(resource);
            let state = persist_failure_state(state);
            Err(SpiPersistFailure::new(
                error_mapper::map(error, FsOperation::PersistTemp, logical_target, None, provider_id)
                    .with_effect_state(persist_effect_state(state)),
                state,
            ))
        }
    }
}

/// Keeps a retained native temporary file and restores it after a
/// pre-publication failure.
fn keep_file(
    slot: &mut Option<native_files::LocalTempFile>,
    provider_id: &str,
) -> Result<native_files::outcome::LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.keep() {
        Ok(result) => Ok(result),
        Err(error) => {
            let (error, resource, _, _, _, state) = error.into_parts_with_state();
            *slot = Some(resource);
            let state = persist_failure_state(state);
            Err(SpiPersistFailure::new(
                error_mapper::map_without_path(error, FsOperation::KeepTemp, "temporary file keep failed", provider_id)
                    .with_effect_state(persist_effect_state(state)),
                state,
            ))
        }
    }
}

/// Keeps a retained native temporary directory and restores it after a
/// pre-publication failure.
fn keep_directory(
    slot: &mut Option<native_files::LocalTempDirectory>,
    provider_id: &str,
) -> Result<native_files::outcome::LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.keep() {
        Ok(result) => Ok(result),
        Err(error) => {
            let (error, resource, _, _, _, state) = error.into_parts_with_state();
            *slot = Some(resource);
            let state = persist_failure_state(state);
            Err(SpiPersistFailure::new(
                error_mapper::map_without_path(
                    error,
                    FsOperation::KeepTemp,
                    "temporary directory keep failed",
                    provider_id,
                )
                .with_effect_state(persist_effect_state(state)),
                state,
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
fn persist_failure_state(state: native_files::outcome::LocalPersistFailureState) -> PersistFailureState {
    match state {
        native_files::outcome::LocalPersistFailureState::NotPublished => PersistFailureState::NotPublished,
        native_files::outcome::LocalPersistFailureState::Published => PersistFailureState::PublishedSourceRetained,
        native_files::outcome::LocalPersistFailureState::Indeterminate => PersistFailureState::Indeterminate,
        _ => PersistFailureState::Indeterminate,
    }
}

/// Maps a portable persistence failure state to its confirmed external effect.
///
/// # Parameters
///
/// - `state`: Source-ownership and destination-publication state after a failed
///   persistence attempt.
///
/// # Returns
///
/// `Unchanged` when no target was published, `Applied` when publication is
/// confirmed, or `Indeterminate` when publication cannot be determined.
/// Converts native temporary persistence state into portable effect state.
#[inline]
const fn persist_effect_state(state: PersistFailureState) -> FsEffectState {
    match state {
        PersistFailureState::NotPublished | PersistFailureState::NotPublishedSourceReleased => FsEffectState::Unchanged,
        PersistFailureState::PublishedSourceRetained | PersistFailureState::PublishedSourceReleased => {
            FsEffectState::Applied
        }
        PersistFailureState::Indeterminate => FsEffectState::Indeterminate,
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

/// Maps temporary-resource cleanup with local provider context.
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
fn cleanup_error(error: native_files::LocalFileError, message: &'static str, provider_id: &str) -> FsError {
    error_mapper::map_without_path(error, FsOperation::CleanupTemp, message, provider_id)
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
fn file_cleanup_error(error: native_files::LocalFileError, provider_id: &str) -> FsError {
    cleanup_error(error, "temporary file cleanup failed", provider_id)
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
fn directory_cleanup_error(error: native_files::LocalFileError, provider_id: &str) -> FsError {
    if error.kind() == native_files::error::LocalFileErrorKind::InvalidPath {
        return FsError::with_source(
            FsErrorKind::NotDirectory,
            FsOperation::CleanupTemp,
            "temporary directory identity no longer names a directory",
            error,
        )
        .with_provider(provider_id);
    }
    cleanup_error(error, "temporary directory cleanup failed", provider_id)
}

#[cfg(test)]
mod tests {
    use qubit_fs::error::FsEffectState;
    use qubit_fs::error::FsError;
    use qubit_fs::error::FsErrorKind;
    use qubit_fs::error::FsOperation;
    use qubit_fs::spi::TempResourceSpi;
    use qubit_fs::temp::PersistFailureState;
    use qubit_local_files::error::LocalFileError;
    use qubit_local_files::error::LocalFileErrorKind;
    use qubit_local_files::error::LocalFileOperation;

    use super::LocalTempResourceSpi;
    use super::file_cleanup_error;
    use super::logical_persist_error;
    use super::persist_effect_state;
    use super::persist_failure_state;
    use super::persist_path_error;
    use super::terminal_persist_error;

    /// Persistence wrappers retain the certainty appropriate to when path
    /// conversion failed.
    #[test]
    fn test_path_failure_wrappers_preserve_publication_certainty() {
        let before_publication = persist_path_error(FsError::new(
            FsErrorKind::InvalidPath,
            FsOperation::PersistTemp,
            "invalid publication target",
        ));
        assert_eq!(PersistFailureState::NotPublished, before_publication.state());
        assert_eq!(FsErrorKind::InvalidPath, before_publication.error().kind());

        let after_publication = logical_persist_error(FsError::new(
            FsErrorKind::InvalidPath,
            FsOperation::PersistTemp,
            "published path is not representable",
        ));
        assert_eq!(PersistFailureState::Indeterminate, after_publication.state());
        assert_eq!(FsErrorKind::InvalidPath, after_publication.error().kind());
    }

    /// Terminal resources reject persistence and cleanup without fabricating
    /// an external side effect.
    #[test]
    fn test_terminal_resources_reject_lifecycle_operations() {
        let failure = terminal_persist_error();
        assert_eq!(PersistFailureState::NotPublished, failure.state());
        assert_eq!(FsErrorKind::InvalidState, failure.error().kind());

        for mut resource in [
            LocalTempResourceSpi::File {
                resource: None,
                rooted: true,
                provider_id: "terminal-file".to_owned(),
                projection: None,
            },
            LocalTempResourceSpi::Directory {
                resource: None,
                rooted: true,
                provider_id: "terminal-directory".to_owned(),
                projection: None,
            },
        ] {
            let error = resource.cleanup().expect_err("terminal resource must reject cleanup");
            assert_eq!(FsErrorKind::InvalidState, error.kind());
            assert_eq!(FsOperation::CleanupTemp, error.operation());
            assert_eq!(None, error.effect_state());
        }
    }

    /// Native failure states and cleanup failures retain their portable
    /// classification and provider identity.
    #[test]
    fn test_native_failure_helpers_preserve_state_and_provider() {
        use qubit_local_files::outcome::LocalPersistFailureState as NativeState;

        for (native, expected, effect) in [
            (
                NativeState::NotPublished,
                PersistFailureState::NotPublished,
                FsEffectState::Unchanged,
            ),
            (
                NativeState::Published,
                PersistFailureState::PublishedSourceRetained,
                FsEffectState::Applied,
            ),
            (
                NativeState::Indeterminate,
                PersistFailureState::Indeterminate,
                FsEffectState::Indeterminate,
            ),
        ] {
            let state = persist_failure_state(native);
            assert_eq!(expected, state);
            assert_eq!(effect, persist_effect_state(state));
        }

        assert_eq!(
            FsEffectState::Unchanged,
            persist_effect_state(PersistFailureState::NotPublishedSourceReleased)
        );
        assert_eq!(
            FsEffectState::Applied,
            persist_effect_state(PersistFailureState::PublishedSourceReleased)
        );

        let error = file_cleanup_error(
            LocalFileError::new(LocalFileErrorKind::PermissionDenied, LocalFileOperation::Cleanup),
            "local-test-provider",
        );
        assert_eq!(FsErrorKind::PermissionDenied, error.kind());
        assert_eq!(FsOperation::CleanupTemp, error.operation());
        assert_eq!(Some("local-test-provider"), error.provider());
    }
}
