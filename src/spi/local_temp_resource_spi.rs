// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
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

pub(crate) enum LocalTempResourceSpi {
    File(Option<native_files::LocalTempFile>, bool),
    Directory(Option<native_files::LocalTempDirectory>, bool),
}
impl LocalTempResourceSpi {
    pub(crate) const fn file(
        value: native_files::LocalTempFile,
        rooted: bool,
    ) -> Self {
        Self::File(Some(value), rooted)
    }
    pub(crate) const fn directory(
        value: native_files::LocalTempDirectory,
        rooted: bool,
    ) -> Self {
        Self::Directory(Some(value), rooted)
    }
}
impl TempResourceSpi for LocalTempResourceSpi {
    fn persist(
        &mut self,
        request: PersistRequest<'_>,
    ) -> Result<PersistOutcome, SpiPersistFailure> {
        let (target, rooted) = match self {
            Self::File(_, rooted) | Self::Directory(_, rooted) => (
                if *rooted {
                    LocalPathMapper::rooted(request.target())
                } else {
                    LocalPathMapper::host(request.target())
                },
                *rooted,
            ),
        };
        let target = target.map_err(|error| {
            SpiPersistFailure::new(error, PersistFailureState::NotPublished)
        })?;
        let options = if request.options().overwrite {
            native_files::LocalPersistOptions::new().with_overwrite()
        } else {
            native_files::LocalPersistOptions::new()
        };
        let result = match self {
            Self::File(slot, _) => {
                let resource = slot.take().ok_or_else(|| {
                    SpiPersistFailure::new(
                        FsError::new(
                            FsErrorKind::InvalidState,
                            FsOperation::PersistTemp,
                            "temporary resource is terminal",
                        ),
                        PersistFailureState::NotPublished,
                    )
                })?;
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
            Self::Directory(slot, _) => {
                let resource = slot.take().ok_or_else(|| {
                    SpiPersistFailure::new(
                        FsError::new(
                            FsErrorKind::InvalidState,
                            FsOperation::PersistTemp,
                            "temporary resource is terminal",
                        ),
                        PersistFailureState::NotPublished,
                    )
                })?;
                match resource.persist(&target) {
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
        .map_err(|error| {
            SpiPersistFailure::new(error, PersistFailureState::Indeterminate)
        })?;
        Ok(PersistOutcome::new(
            logical,
            AchievedAtomicity::Atomic,
            PublicationMethod::AtomicRename,
        ))
    }
    fn keep(&mut self) -> FsResult<()> {
        match self {
            Self::File(value, _) => {
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
            Self::Directory(value, _) => {
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
    fn cleanup(&mut self) -> FsResult<()> {
        match self {
            Self::File(value, _) => value
                .as_mut()
                .ok_or_else(|| {
                    FsError::new(
                        FsErrorKind::InvalidState,
                        FsOperation::CleanupTemp,
                        "temporary resource is terminal",
                    )
                })?
                .cleanup()
                .map_err(|error| {
                    FsError::with_source(
                        FsErrorKind::Io,
                        FsOperation::CleanupTemp,
                        "temporary file cleanup failed",
                        error,
                    )
                }),
            Self::Directory(value, _) => value
                .as_mut()
                .ok_or_else(|| {
                    FsError::new(
                        FsErrorKind::InvalidState,
                        FsOperation::CleanupTemp,
                        "temporary resource is terminal",
                    )
                })?
                .cleanup()
                .map_err(|error| {
                    FsError::with_source(
                        FsErrorKind::Io,
                        FsOperation::CleanupTemp,
                        "temporary directory cleanup failed",
                        error,
                    )
                }),
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
    match error.stage() {
        native_files::LocalPersistStage::ResolveTarget
        | native_files::LocalPersistStage::PrepareParent => {
            PersistFailureState::NotPublished
        }
        native_files::LocalPersistStage::InstallDestination
            if error.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            PersistFailureState::NotPublished
        }
        native_files::LocalPersistStage::InstallDestination => {
            PersistFailureState::Indeterminate
        }
        _ => PersistFailureState::Indeterminate,
    }
}
