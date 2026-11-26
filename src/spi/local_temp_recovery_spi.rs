// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cleanup-only SPI for recovery after a temporary-resource creation failure.

use qubit_fs::error::FsEffectState;
use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_fs::spi::PersistRequest;
use qubit_fs::spi::SpiPersistFailure;
use qubit_fs::spi::TempResourceSpi;
use qubit_fs::temp::PersistFailureState;
use qubit_fs::temp::PersistOutcome;
use qubit_local_files::LocalTempRecovery;
use qubit_local_files::path::LocalFileSystemScope;

use crate::path::local_path_mapper;
use crate::spi::error_mapper;

/// Cleanup-only provider session for a partially created temporary object.
pub(crate) struct LocalTempRecoverySpi {
    /// Native cleanup authority retained until cleanup succeeds.
    owner: Option<LocalTempRecovery>,
    /// Whether cleanup paths must be projected relative to a Rooted authority.
    rooted: bool,
    /// Provider identity attached to cleanup failures.
    provider_id: String,
}

impl LocalTempRecoverySpi {
    /// Wraps a native recovery owner in a cleanup-only provider session.
    ///
    /// # Parameters
    ///
    /// - `owner`: Native authority for retrying cleanup.
    /// - `rooted`: Whether the owner belongs to a Rooted filesystem.
    /// - `provider_id`: Provider identity attached to cleanup failures.
    pub(crate) fn new(owner: LocalTempRecovery, rooted: bool, provider_id: String) -> Self {
        Self {
            owner: Some(owner),
            rooted,
            provider_id,
        }
    }
}

impl TempResourceSpi for LocalTempRecoverySpi {
    /// Rejects publication because failed creation permits cleanup only.
    ///
    /// # Parameters
    ///
    /// - `_request`: Publication request, which this recovery session rejects.
    ///
    /// # Errors
    ///
    /// Always returns `InvalidState` while retaining cleanup ownership.
    fn persist(&mut self, _: PersistRequest<'_>) -> Result<PersistOutcome, SpiPersistFailure> {
        Err(SpiPersistFailure::new(
            FsError::new(
                FsErrorKind::InvalidState,
                FsOperation::PersistTemp,
                "failed temporary creation permits cleanup only",
            )
            .with_provider(&self.provider_id)
            .with_effect_state(FsEffectState::PartiallyApplied),
            PersistFailureState::NotPublishedSourceCleanupRequired,
        ))
    }

    /// Rejects keeping the partial object because failed creation permits
    /// cleanup only.
    ///
    /// # Returns
    ///
    /// A persist outcome, which this recovery session never produces.
    ///
    /// # Errors
    ///
    /// Always returns `InvalidState` while retaining cleanup ownership.
    fn keep(&mut self) -> Result<PersistOutcome, SpiPersistFailure> {
        Err(SpiPersistFailure::new(
            FsError::new(
                FsErrorKind::InvalidState,
                FsOperation::PersistTemp,
                "failed temporary creation permits cleanup only",
            )
            .with_provider(&self.provider_id)
            .with_effect_state(FsEffectState::PartiallyApplied),
            PersistFailureState::NotPublishedSourceCleanupRequired,
        ))
    }

    /// Retries cleanup while retaining native authority after a failure.
    ///
    /// # Returns
    ///
    /// `Ok(())` when cleanup already completed or succeeds.
    ///
    /// # Errors
    ///
    /// Returns the mapped native cleanup failure while retaining the owner.
    fn cleanup(&mut self) -> FsResult<()> {
        let Some(owner) = self.owner.as_mut() else {
            return Ok(());
        };
        owner.cleanup().map_err(|error| {
            let operation = FsOperation::CleanupTemp;
            let logical = local_path_mapper::logical(
                if self.rooted {
                    LocalFileSystemScope::Rooted
                } else {
                    LocalFileSystemScope::Host
                },
                owner.path(),
                operation,
            );
            let mapped = error_mapper::map_without_path(
                error,
                operation,
                "temporary creation recovery cleanup failed",
                &self.provider_id,
            );
            match logical {
                Ok(path) => mapped.with_path(path),
                Err(_) => mapped,
            }
        })
    }
}
