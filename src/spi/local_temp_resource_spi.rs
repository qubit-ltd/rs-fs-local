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
use qubit_local_files::LocalFileError;
use qubit_local_files::LocalTempDirectory;
use qubit_local_files::LocalTempFile;
use qubit_local_files::error::LocalFileEffectState;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalPersistErrorParts;
use qubit_local_files::options::LocalPersistOptions;
use qubit_local_files::outcome::LocalPersistCleanupState;
use qubit_local_files::outcome::LocalPersistFailureState;
use qubit_local_files::outcome::LocalPersistMethod;
use qubit_local_files::outcome::LocalPersistOutcome;
use qubit_local_files::outcome::LocalTempSourceState;
use qubit_local_files::path::LocalFileSystemScope;

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
        resource: Option<LocalTempFile>,
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
        resource: Option<LocalTempDirectory>,
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
        value: LocalTempFile,
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
        value: LocalTempDirectory,
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

    /// Rejects publication before target conversion unless the retained native
    /// resource still owns its source. The failure preserves current authority
    /// and reports that this rejected call did not publish a target.
    fn require_owned(&self, operation: FsOperation) -> Result<(), SpiPersistFailure> {
        let source = match self {
            Self::File { resource, .. } => resource.as_ref().map(LocalTempFile::source_state),
            Self::Directory { resource, .. } => resource.as_ref().map(LocalTempDirectory::source_state),
        }
        .unwrap_or(LocalTempSourceState::Released);
        if source == LocalTempSourceState::Owned {
            return Ok(());
        }
        let state = persist_failure_state(LocalPersistFailureState::NotPublished, source);
        Err(SpiPersistFailure::new(
            FsError::new(
                FsErrorKind::InvalidState,
                operation,
                "temporary resource no longer owns its source",
            )
            .with_effect_state(persist_effect_state(state)),
            state,
        ))
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
    /// Returns `InvalidState` before conversion when source authority is not
    /// owned, or `InvalidPath` when the logical target cannot be converted to
    /// the resource's native authority.
    fn target(&self, target: &LogicalPath) -> Result<(PathBuf, bool), SpiPersistFailure> {
        self.require_owned(FsOperation::PersistTemp)?;
        let rooted = match self {
            Self::File { rooted, .. } | Self::Directory { rooted, .. } => *rooted,
        };
        let target = local_path_mapper::native(
            if rooted {
                LocalFileSystemScope::Rooted
            } else {
                LocalFileSystemScope::Host
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
        self.require_owned(FsOperation::KeepTemp)?;
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
                LocalFileSystemScope::Rooted
            } else {
                LocalFileSystemScope::Host
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
                resource, provider_id, ..
            } => {
                let resource = resource.as_mut().ok_or_else(terminal_cleanup_error)?;
                resource
                    .cleanup()
                    .map_err(|error| file_cleanup_error(error, resource.source_state(), provider_id))
            }
            Self::Directory {
                resource, provider_id, ..
            } => {
                let resource = resource.as_mut().ok_or_else(terminal_cleanup_error)?;
                resource
                    .cleanup()
                    .map_err(|error| directory_cleanup_error(error, resource.source_state(), provider_id))
            }
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
fn persist_options(options: &PersistOptions) -> LocalPersistOptions {
    let mut native = LocalPersistOptions::new();
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
fn map_persist_outcome(result: LocalPersistOutcome, logical: LogicalPath) -> PersistOutcome {
    PersistOutcome::new(
        logical,
        if result.atomic() {
            AchievedAtomicity::Atomic
        } else {
            AchievedAtomicity::NonAtomic
        },
        match result.method() {
            LocalPersistMethod::AtomicRename => PublicationMethod::AtomicRename,
            _ => PublicationMethod::Direct,
        },
    )
    .with_cleanup_state(match result.cleanup_state() {
        LocalPersistCleanupState::Complete => PersistCleanupState::Complete,
        LocalPersistCleanupState::ResidualSandbox => PersistCleanupState::ResidualTemporaryContainer,
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
    slot: &mut Option<LocalTempFile>,
    target: &Path,
    logical_target: &LogicalPath,
    options: LocalPersistOptions,
    provider_id: &str,
) -> Result<LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.persist_with(target, options) {
        Ok(result) => Ok(result),
        Err(error) => {
            let LocalPersistErrorParts {
                error,
                resource,
                state,
                source_state,
                ..
            } = error.into_parts();
            *slot = Some(resource);
            let state = persist_failure_state(state, source_state);
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
    slot: &mut Option<LocalTempDirectory>,
    target: &Path,
    logical_target: &LogicalPath,
    options: LocalPersistOptions,
    provider_id: &str,
) -> Result<LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.persist_with(target, options) {
        Ok(result) => Ok(result),
        Err(error) => {
            let LocalPersistErrorParts {
                error,
                resource,
                state,
                source_state,
                ..
            } = error.into_parts();
            *slot = Some(resource);
            let state = persist_failure_state(state, source_state);
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
fn keep_file(slot: &mut Option<LocalTempFile>, provider_id: &str) -> Result<LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.keep() {
        Ok(result) => Ok(result),
        Err(error) => {
            let LocalPersistErrorParts {
                error,
                resource,
                state,
                source_state,
                ..
            } = error.into_parts();
            *slot = Some(resource);
            let state = persist_failure_state(state, source_state);
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
    slot: &mut Option<LocalTempDirectory>,
    provider_id: &str,
) -> Result<LocalPersistOutcome, SpiPersistFailure> {
    let resource = slot.take().ok_or_else(terminal_persist_error)?;
    match resource.keep() {
        Ok(result) => Ok(result),
        Err(error) => {
            let LocalPersistErrorParts {
                error,
                resource,
                state,
                source_state,
                ..
            } = error.into_parts();
            *slot = Some(resource);
            let state = persist_failure_state(state, source_state);
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

/// Converts the current publication and source-authority facts to portable
/// state.
///
/// Unknown future combinations and the invalid Published + Owned combination
/// conservatively return `Indeterminate`.
#[inline]
fn persist_failure_state(publication: LocalPersistFailureState, source: LocalTempSourceState) -> PersistFailureState {
    use qubit_local_files::outcome::LocalPersistFailureState as Publication;
    use qubit_local_files::outcome::LocalTempSourceState as Source;

    match (publication, source) {
        (Publication::NotPublished, Source::Owned) => PersistFailureState::NotPublished,
        (Publication::NotPublished, Source::Released) => PersistFailureState::NotPublishedSourceReleased,
        (Publication::NotPublished, Source::Indeterminate) => PersistFailureState::NotPublishedSourceIndeterminate,
        (Publication::NotPublished, Source::CleanupRequired) => PersistFailureState::NotPublishedSourceCleanupRequired,
        (Publication::Published, Source::CleanupRequired) => PersistFailureState::PublishedSourceRetained,
        (Publication::Published, Source::Released) => PersistFailureState::PublishedSourceReleased,
        (Publication::Published, Source::Indeterminate) => PersistFailureState::PublishedSourceIndeterminate,
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
        PersistFailureState::NotPublished
        | PersistFailureState::NotPublishedSourceReleased
        | PersistFailureState::NotPublishedSourceIndeterminate
        | PersistFailureState::NotPublishedSourceCleanupRequired => FsEffectState::Unchanged,
        PersistFailureState::PublishedSourceRetained
        | PersistFailureState::PublishedSourceReleased
        | PersistFailureState::PublishedSourceIndeterminate => FsEffectState::Applied,
        PersistFailureState::Indeterminate => FsEffectState::Indeterminate,
    }
}

/// Builds the failure returned for an already-terminal temporary resource.
///
/// # Returns
///
/// An `InvalidState` failure preserving the released source state.
#[inline(always)]
fn terminal_persist_error() -> SpiPersistFailure {
    SpiPersistFailure::new(
        FsError::new(
            FsErrorKind::InvalidState,
            FsOperation::PersistTemp,
            "temporary resource is terminal",
        ),
        PersistFailureState::NotPublishedSourceReleased,
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
/// A failure retaining confirmed publication and released source ownership,
/// with `Applied` effect even though its logical target cannot be represented.
#[inline(always)]
fn logical_persist_error(error: FsError) -> SpiPersistFailure {
    SpiPersistFailure::new(
        error.with_effect_state(FsEffectState::Applied),
        PersistFailureState::PublishedSourceReleased,
    )
}

/// Maps temporary-resource cleanup with local provider context.
///
/// # Parameters
///
/// - `error`: Native cleanup failure, retained as the cause.
/// - `source`: Retained resource authority after the failed cleanup.
/// - `message`: Static context identifying the resource kind.
///
/// # Returns
///
/// A facade cleanup error.
#[inline(always)]
fn cleanup_error(
    error: LocalFileError,
    source: LocalTempSourceState,
    message: &'static str,
    provider_id: &str,
) -> FsError {
    if source == LocalTempSourceState::Indeterminate {
        // Losing source identity prevents deletion; preserve any stronger native
        // effect claim independently from the now-uncertain source authority.
        let effect = match error.effect_state() {
            None | Some(LocalFileEffectState::Unchanged) => FsEffectState::Unchanged,
            Some(LocalFileEffectState::PartiallyApplied) => FsEffectState::PartiallyApplied,
            Some(LocalFileEffectState::Applied) => FsEffectState::Applied,
            _ => FsEffectState::Indeterminate,
        };
        return FsError::with_source(
            FsErrorKind::Indeterminate,
            FsOperation::CleanupTemp,
            "temporary resource source authority cannot be established; cleanup failed",
            error,
        )
        .with_effect_state(effect)
        .with_provider(provider_id);
    }
    error_mapper::map_without_path(error, FsOperation::CleanupTemp, message, provider_id)
}

/// Rejects cleanup when the adapter has already released its native resource.
fn terminal_cleanup_error() -> FsError {
    FsError::new(
        FsErrorKind::InvalidState,
        FsOperation::CleanupTemp,
        "temporary resource is terminal",
    )
}

/// Maps a native temporary-file cleanup failure.
///
/// # Parameters
///
/// - `error`: Native cleanup failure, retained as the cause.
/// - `source`: Retained resource authority after the failed cleanup.
///
/// # Returns
///
/// A facade cleanup error identifying temporary-file cleanup.
#[inline(always)]
fn file_cleanup_error(error: LocalFileError, source: LocalTempSourceState, provider_id: &str) -> FsError {
    cleanup_error(error, source, "temporary file cleanup failed", provider_id)
}

/// Maps a native temporary-directory cleanup failure.
///
/// # Parameters
///
/// - `error`: Native cleanup failure, retained as the cause.
/// - `source`: Retained resource authority after the failed cleanup.
///
/// # Returns
///
/// A facade cleanup error identifying temporary-directory cleanup.
#[inline(always)]
fn directory_cleanup_error(error: LocalFileError, source: LocalTempSourceState, provider_id: &str) -> FsError {
    if source != LocalTempSourceState::Indeterminate && error.kind() == LocalFileErrorKind::InvalidPath {
        return FsError::with_source(
            FsErrorKind::NotDirectory,
            FsOperation::CleanupTemp,
            "temporary directory identity no longer names a directory",
            error,
        )
        .with_provider(provider_id);
    }
    cleanup_error(error, source, "temporary directory cleanup failed", provider_id)
}

#[cfg(test)]
mod tests {
    use qubit_fs::Path;
    use qubit_fs::error::FsEffectState;
    use qubit_fs::error::FsError;
    use qubit_fs::error::FsErrorKind;
    use qubit_fs::error::FsOperation;
    use qubit_fs::spi::TempResourceSpi;
    use qubit_fs::temp::PersistFailureState;
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::error::LocalFileError;
    use qubit_local_files::error::LocalFileErrorKind;
    use qubit_local_files::error::LocalFileOperation;
    #[cfg(unix)]
    use qubit_local_files::options::LocalPersistOptions;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTempFileOptions;
    #[cfg(unix)]
    use qubit_local_files::policy::LocalDurabilityRequirement;
    #[cfg(unix)]
    use qubit_local_files::test_support::install_test_fault;

    use super::LocalTempResourceSpi;
    use super::directory_cleanup_error;
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
        assert_eq!(PersistFailureState::PublishedSourceReleased, after_publication.state());
        assert_eq!(Some(FsEffectState::Applied), after_publication.error().effect_state());
        assert_eq!(FsErrorKind::InvalidPath, after_publication.error().kind());
    }

    /// Terminal resources reject persistence and cleanup without fabricating
    /// an external side effect.
    #[test]
    fn test_terminal_resources_reject_lifecycle_operations() {
        let failure = terminal_persist_error();
        assert_eq!(PersistFailureState::NotPublishedSourceReleased, failure.state());
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

    /// Native source uncertainty must survive an invalid logical target even
    /// when a caller reaches the SPI directly without the facade guard.
    #[test]
    fn test_direct_spi_replacement_rejects_invalid_target_without_reset() {
        let parent = tempfile::tempdir().expect("test parent");
        let filesystem = LocalFileSystem::host().expect("host filesystem");
        let temporary = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
            .expect("temporary file");
        let source = temporary.path().to_path_buf();
        std::fs::rename(&source, parent.path().join("original")).expect("retain original file");
        std::fs::write(&source, b"replacement").expect("replacement file");
        let error = temporary
            .persist(parent.path().join("published"))
            .expect_err("replacement rejection");
        let mut adapter = LocalTempResourceSpi::file(error.into_parts().resource, false, "test".to_owned(), None);
        assert_direct_spi_rejected(&mut adapter, PersistFailureState::NotPublishedSourceIndeterminate);
        adapter.cleanup().expect_err("uncertain source cannot be cleaned");
        assert_direct_spi_rejected(&mut adapter, PersistFailureState::NotPublishedSourceIndeterminate);
        drop(adapter);
        assert_eq!(
            b"replacement",
            std::fs::read(source).expect("replacement survives").as_slice()
        );
    }

    /// Released native directory authority must survive an invalid target.
    #[test]
    fn test_direct_spi_released_source_rejects_invalid_target_without_reset() {
        let parent = tempfile::tempdir().expect("test parent");
        let filesystem = LocalFileSystem::host().expect("host filesystem");
        let mut temporary = filesystem
            .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(parent.path()))
            .expect("temporary directory");
        temporary.cleanup().expect("release source");
        let mut adapter = LocalTempResourceSpi::directory(temporary, false, "test".to_owned(), None);
        assert_direct_spi_rejected(&mut adapter, PersistFailureState::NotPublishedSourceReleased);
    }

    /// A published native file with residual sandbox ownership maps a rejected
    /// new SPI publication to known Unchanged without restoring source
    /// ownership.
    #[cfg(unix)]
    #[test]
    fn test_direct_spi_cleanup_required_rejects_invalid_target_without_reset() {
        let parent = tempfile::tempdir().expect("test parent");
        let filesystem = LocalFileSystem::host().expect("host filesystem");
        let temporary = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
            .expect("temporary file");
        let fault = install_test_fault("temp-file-parent-sync").expect("sync fault");
        let error = temporary
            .persist_with(
                parent.path().join("published"),
                LocalPersistOptions::new().with_durability(LocalDurabilityRequirement::Required),
            )
            .expect_err("injected post-publication sync failure");
        drop(fault);
        let mut adapter = LocalTempResourceSpi::file(error.into_parts().resource, false, "test".to_owned(), None);
        assert_direct_spi_rejected(&mut adapter, PersistFailureState::NotPublishedSourceCleanupRequired);
        adapter.cleanup().expect("residual sandbox cleanup");
        assert_direct_spi_rejected(&mut adapter, PersistFailureState::NotPublishedSourceReleased);
        drop(adapter);
        assert!(parent.path().join("published").is_file());
    }

    /// Checks invalid target, valid target, and keep rejections against
    /// retained authority, including the known absence of a new publication
    /// effect.
    fn assert_direct_spi_rejected(adapter: &mut LocalTempResourceSpi, expected: PersistFailureState) {
        for target in ["relative", "/target"] {
            let target = Path::parse(target).expect("logical test target");
            let failure = adapter
                .target(&target)
                .expect_err("non-owned source rejects persist target");
            assert_eq!(expected, failure.state());
            assert_eq!(FsErrorKind::InvalidState, failure.error().kind());
            assert_eq!(Some(FsEffectState::Unchanged), failure.error().effect_state());
        }
        let failure = adapter.keep().expect_err("non-owned source rejects keep");
        assert_eq!(expected, failure.state());
        assert_eq!(FsOperation::KeepTemp, failure.error().operation());
        assert_eq!(Some(FsEffectState::Unchanged), failure.error().effect_state());
    }

    /// Cleanup source authority does not erase known partial effects or turn
    /// ordinary Owned/CleanupRequired failures into uncertain source ownership.
    #[test]
    fn test_cleanup_mapping_preserves_effect_independently_from_source() {
        use qubit_local_files::outcome::LocalTempSourceState;
        for source in [
            LocalTempSourceState::Owned,
            LocalTempSourceState::CleanupRequired,
            LocalTempSourceState::Indeterminate,
        ] {
            for map_error in [file_cleanup_error, directory_cleanup_error] {
                let partial = map_error(
                    LocalFileError::new(LocalFileErrorKind::PublicationIncomplete, LocalFileOperation::Cleanup),
                    source,
                    "cleanup-test",
                );
                assert_eq!(Some(FsEffectState::PartiallyApplied), partial.effect_state());
                assert_eq!(
                    source == LocalTempSourceState::Indeterminate,
                    partial.has_indeterminate_effect()
                );
                let ordinary = map_error(
                    LocalFileError::new(LocalFileErrorKind::PermissionDenied, LocalFileOperation::Cleanup),
                    source,
                    "cleanup-test",
                );
                if source == LocalTempSourceState::Indeterminate {
                    assert_eq!(FsErrorKind::Indeterminate, ordinary.kind());
                    assert_eq!(Some(FsEffectState::Unchanged), ordinary.effect_state());
                } else {
                    assert_eq!(FsErrorKind::PermissionDenied, ordinary.kind());
                    assert_eq!(None, ordinary.effect_state());
                }
                assert_eq!(Some("cleanup-test"), ordinary.provider());
            }
        }
    }

    /// Native failure states and cleanup failures retain their portable
    /// classification and provider identity.
    #[test]
    fn test_native_failure_helpers_preserve_state_and_provider() {
        use qubit_local_files::outcome::LocalPersistFailureState as Publication;
        use qubit_local_files::outcome::LocalTempSourceState as Source;

        for (publication, source, expected, effect) in [
            (
                Publication::NotPublished,
                Source::Owned,
                PersistFailureState::NotPublished,
                FsEffectState::Unchanged,
            ),
            (
                Publication::NotPublished,
                Source::Released,
                PersistFailureState::NotPublishedSourceReleased,
                FsEffectState::Unchanged,
            ),
            (
                Publication::NotPublished,
                Source::Indeterminate,
                PersistFailureState::NotPublishedSourceIndeterminate,
                FsEffectState::Unchanged,
            ),
            (
                Publication::NotPublished,
                Source::CleanupRequired,
                PersistFailureState::NotPublishedSourceCleanupRequired,
                FsEffectState::Unchanged,
            ),
            (
                Publication::Published,
                Source::CleanupRequired,
                PersistFailureState::PublishedSourceRetained,
                FsEffectState::Applied,
            ),
            (
                Publication::Published,
                Source::Released,
                PersistFailureState::PublishedSourceReleased,
                FsEffectState::Applied,
            ),
            (
                Publication::Published,
                Source::Indeterminate,
                PersistFailureState::PublishedSourceIndeterminate,
                FsEffectState::Applied,
            ),
            (
                Publication::Published,
                Source::Owned,
                PersistFailureState::Indeterminate,
                FsEffectState::Indeterminate,
            ),
        ] {
            let state = persist_failure_state(publication, source);
            assert_eq!(expected, state);
            assert_eq!(effect, persist_effect_state(state));
        }
        for source in [
            Source::Owned,
            Source::Released,
            Source::CleanupRequired,
            Source::Indeterminate,
        ] {
            let state = persist_failure_state(Publication::Indeterminate, source);
            assert_eq!(PersistFailureState::Indeterminate, state);
            assert_eq!(FsEffectState::Indeterminate, persist_effect_state(state));
        }

        let error = file_cleanup_error(
            LocalFileError::new(LocalFileErrorKind::PermissionDenied, LocalFileOperation::Cleanup),
            Source::Owned,
            "local-test-provider",
        );
        assert_eq!(FsErrorKind::PermissionDenied, error.kind());
        assert_eq!(FsOperation::CleanupTemp, error.operation());
        assert_eq!(Some("local-test-provider"), error.provider());
    }
}
