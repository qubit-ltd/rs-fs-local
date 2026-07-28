// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local temporary-resource lifecycle session.

use std::{
    fs,
    path::PathBuf,
};

use qubit_fs::{
    AchievedAtomicity,
    FsError,
    FsOperation,
    FsPath,
    FsResult,
    PersistFailure,
    PersistFailureState,
    PersistOptions,
    PersistOutcome,
    PublicationMethod,
    TempResourceSession,
};
use qubit_local_files::backend::rename;

use crate::LocalFileSystem;

/// Owns cleanup responsibility for one host-local temporary resource.
pub(crate) struct LocalTempResourceSession {
    source: PathBuf,
    source_path: FsPath,
    directory: bool,
    active: bool,
}

impl LocalTempResourceSession {
    /// Creates a session that owns an already-created temporary resource.
    pub(crate) fn new(
        source: PathBuf,
        source_path: FsPath,
        directory: bool,
    ) -> Self {
        Self {
            source,
            source_path,
            directory,
            active: true,
        }
    }

    /// Converts an I/O failure into a source-aware filesystem error.
    fn error(&self, operation: FsOperation, error: std::io::Error) -> FsError {
        FsError::from_io(error, operation)
            .with_path(self.source_path.clone())
            .with_provider(LocalFileSystem::provider_id())
    }
}

impl TempResourceSession for LocalTempResourceSession {
    fn cleanup(&mut self) -> FsResult<()> {
        if !self.active {
            return Ok(());
        }
        let result = if self.directory {
            fs::remove_dir_all(&self.source)
        } else {
            fs::remove_file(&self.source)
        };
        match result {
            Ok(()) => {
                self.active = false;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.active = false;
                Ok(())
            }
            Err(error) => Err(self.error(FsOperation::CleanupTemp, error)),
        }
    }

    fn keep(&mut self) -> FsResult<()> {
        self.active = false;
        Ok(())
    }

    fn persist(
        &mut self,
        target: &FsPath,
        options: PersistOptions,
    ) -> Result<PersistOutcome, PersistFailure> {
        let target_native =
            LocalFileSystem::native_path(FsOperation::PersistTemp, target)
                .map_err(|error| {
                    PersistFailure::new(
                        error
                            .with_path(self.source_path.clone())
                            .with_target(target.clone()),
                        PersistFailureState::NotPublished,
                    )
                })?;
        let result = if options.overwrite {
            rename::move_path(&self.source, &target_native)
        } else {
            rename::move_path_without_replacing(&self.source, &target_native)
        };
        result.map_err(|error| {
            PersistFailure::new(
                self.error(FsOperation::PersistTemp, error)
                    .with_target(target.clone()),
                PersistFailureState::NotPublished,
            )
        })?;
        self.active = false;
        Ok(PersistOutcome::new(
            target.clone(),
            AchievedAtomicity::Atomic,
            PublicationMethod::AtomicRename,
        ))
    }
}
