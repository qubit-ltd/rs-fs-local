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

use qubit_fs::spi::{
    PersistRequest,
    SpiPersistFailure,
    TempResourceSpi,
};
use qubit_fs::{
    AchievedAtomicity,
    FsError,
    FsErrorKind,
    FsOperation,
    FsResult,
    PersistFailureState,
    PersistOutcome,
    PublicationMethod,
};
use qubit_local_files as native_files;

use crate::path::LocalPathMapper;

/// Adapts one native temporary resource while retaining its authority mode.
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
    pub(crate) const fn file(
        value: native_files::LocalTempFile,
        rooted: bool,
    ) -> Self {
        Self::File {
            resource: Some(value),
            rooted,
        }
    }

    /// Wraps a host- or rooted-authority native temporary directory.
    pub(crate) const fn directory(
        value: native_files::LocalTempDirectory,
        rooted: bool,
    ) -> Self {
        Self::Directory {
            resource: Some(value),
            rooted,
        }
    }
}
impl TempResourceSpi for LocalTempResourceSpi {
    /// Persists the native resource with the caller's replacement policy.
    fn persist(
        &mut self,
        request: PersistRequest<'_>,
    ) -> Result<PersistOutcome, SpiPersistFailure> {
        let (target, rooted) = match self {
            Self::File { rooted, .. } | Self::Directory { rooted, .. } => (
                if *rooted {
                    LocalPathMapper::rooted(request.target())
                } else {
                    LocalPathMapper::host(request.target())
                },
                *rooted,
            ),
        };
        let target = target.map_err(persist_path_error)?;
        let options = if request.options().overwrite {
            native_files::LocalPersistOptions::new().with_overwrite()
        } else {
            native_files::LocalPersistOptions::new()
        };
        let result = match self {
            Self::File { resource: slot, .. } => {
                let resource =
                    slot.take().ok_or_else(terminal_persist_error)?;
                match resource.persist_with(&target, options) {
                    Ok(result) => Ok(result),
                    Err(error) => {
                        let state = native_persist_failure_state(&error);
                        let (io, resource, _, _, _) = error.into_parts();
                        *slot = Some(resource);
                        Err(SpiPersistFailure::new(
                            FsError::with_source(
                                FsErrorKind::Io,
                                FsOperation::PersistTemp,
                                "temporary file persistence failed",
                                io,
                            ),
                            state,
                        ))
                    }
                }
            }
            Self::Directory { resource: slot, .. } => {
                let resource =
                    slot.take().ok_or_else(terminal_persist_error)?;
                match resource.persist_with(&target, options) {
                    Ok(result) => Ok(result),
                    Err(error) => {
                        let state = native_persist_failure_state(&error);
                        let (io, resource, _, _, _) = error.into_parts();
                        *slot = Some(resource);
                        Err(SpiPersistFailure::new(
                            FsError::with_source(
                                FsErrorKind::Io,
                                FsOperation::PersistTemp,
                                "temporary directory persistence failed",
                                io,
                            ),
                            state,
                        ))
                    }
                }
            }
        }?;
        let logical = if rooted {
            LocalPathMapper::rooted_logical(&result)
        } else {
            LocalPathMapper::host_logical(&result)
        }
        .map_err(logical_persist_error)?;
        Ok(PersistOutcome::new(
            logical,
            AchievedAtomicity::Atomic,
            PublicationMethod::AtomicRename,
        ))
    }
    /// Releases cleanup ownership without publishing the native resource.
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

/// Maps native persistence context to the facade recovery state.
///
/// Target resolution and parent preparation fail before publication. An
/// installation conflict also proves that the source remains unpublished;
/// every other installation error leaves the native namespace indeterminate.
fn native_persist_failure_state<T>(
    error: &native_files::LocalPersistError<T>,
) -> PersistFailureState {
    persist_failure_state(error.stage(), error.kind())
}

fn persist_failure_state(
    stage: native_files::LocalPersistStage,
    kind: std::io::ErrorKind,
) -> PersistFailureState {
    match stage {
        native_files::LocalPersistStage::ResolveTarget
        | native_files::LocalPersistStage::PrepareParent => {
            PersistFailureState::NotPublished
        }
        native_files::LocalPersistStage::InstallDestination
            if kind == std::io::ErrorKind::AlreadyExists =>
        {
            PersistFailureState::NotPublished
        }
        native_files::LocalPersistStage::InstallDestination => {
            PersistFailureState::Indeterminate
        }
        _ => PersistFailureState::Indeterminate,
    }
}

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

fn persist_path_error(error: FsError) -> SpiPersistFailure {
    SpiPersistFailure::new(error, PersistFailureState::NotPublished)
}

fn logical_persist_error(error: FsError) -> SpiPersistFailure {
    SpiPersistFailure::new(error, PersistFailureState::Indeterminate)
}

fn cleanup_error(error: std::io::Error, message: &'static str) -> FsError {
    FsError::with_source(
        FsErrorKind::Io,
        FsOperation::CleanupTemp,
        message,
        error,
    )
}

fn file_cleanup_error(error: std::io::Error) -> FsError {
    cleanup_error(error, "temporary file cleanup failed")
}

fn directory_cleanup_error(error: std::io::Error) -> FsError {
    cleanup_error(error, "temporary directory cleanup failed")
}

#[cfg(test)]
mod tests {
    use qubit_fs::{
        FsErrorKind,
        FsOperation,
        spi::TempResourceSpi,
    };
    use qubit_local_files::LocalPersistStage;

    use super::{
        LocalTempResourceSpi,
        cleanup_error,
        directory_cleanup_error,
        file_cleanup_error,
        logical_persist_error,
        persist_failure_state,
        persist_path_error,
        terminal_persist_error,
    };

    #[test]
    fn classifies_persistence_stages_and_helper_errors() {
        assert_eq!(
            qubit_fs::PersistFailureState::NotPublished,
            persist_failure_state(
                LocalPersistStage::ResolveTarget,
                std::io::ErrorKind::Other
            )
        );
        assert_eq!(
            qubit_fs::PersistFailureState::NotPublished,
            persist_failure_state(
                LocalPersistStage::PrepareParent,
                std::io::ErrorKind::Other
            )
        );
        assert_eq!(
            qubit_fs::PersistFailureState::NotPublished,
            persist_failure_state(
                LocalPersistStage::InstallDestination,
                std::io::ErrorKind::AlreadyExists
            )
        );
        assert_eq!(
            qubit_fs::PersistFailureState::Indeterminate,
            persist_failure_state(
                LocalPersistStage::InstallDestination,
                std::io::ErrorKind::Other
            )
        );
        assert_eq!(
            FsErrorKind::InvalidState,
            terminal_persist_error().error().kind()
        );
        assert_eq!(
            qubit_fs::PersistFailureState::NotPublished,
            persist_path_error(qubit_fs::FsError::new(
                FsErrorKind::InvalidPath,
                FsOperation::PersistTemp,
                "test",
            ))
            .state()
        );
        assert_eq!(
            qubit_fs::PersistFailureState::Indeterminate,
            logical_persist_error(qubit_fs::FsError::new(
                FsErrorKind::InvalidPath,
                FsOperation::PersistTemp,
                "test",
            ))
            .state()
        );
        assert_eq!(
            FsErrorKind::Io,
            cleanup_error(
                std::io::Error::other("test failure"),
                "test cleanup failure",
            )
            .kind()
        );
        assert_eq!(
            FsErrorKind::Io,
            file_cleanup_error(std::io::Error::other("test failure")).kind()
        );
        assert_eq!(
            FsErrorKind::Io,
            directory_cleanup_error(std::io::Error::other("test failure"))
                .kind()
        );
    }

    #[test]
    fn terminal_file_resource_rejects_keep_and_cleanup() {
        let mut resource = LocalTempResourceSpi::File {
            resource: None,
            rooted: false,
        };
        assert_eq!(
            FsErrorKind::InvalidState,
            resource
                .keep()
                .expect_err("terminal resource cannot be kept")
                .kind()
        );
        assert_eq!(
            FsErrorKind::InvalidState,
            resource
                .cleanup()
                .expect_err("terminal resource cannot be cleaned")
                .kind()
        );
    }

    #[test]
    fn terminal_directory_resource_rejects_keep_and_cleanup() {
        let mut resource = LocalTempResourceSpi::Directory {
            resource: None,
            rooted: true,
        };
        assert_eq!(
            FsErrorKind::InvalidState,
            resource
                .keep()
                .expect_err("terminal resource cannot be kept")
                .kind()
        );
        assert_eq!(
            FsErrorKind::InvalidState,
            resource
                .cleanup()
                .expect_err("terminal resource cannot be cleaned")
                .kind()
        );
    }
}
