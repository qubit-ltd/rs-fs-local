// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Native outcome conversion helpers.

use qubit_fs::spi::{
    CopyAttempt,
    CopyDeclineReason,
};
use qubit_fs::{
    AchievedAtomicity,
    CopyFailureState,
    CopyMethod,
    CopyOutcome,
    CopyStats,
    FileKind,
    FileMetadata,
    MetadataPreservePolicy,
    PublicationMethod,
    RenameFailureState,
    RenameOutcome,
};
use qubit_local_files as native_files;

/// Converts a native metadata snapshot to portable facade metadata.
///
/// # Parameters
///
/// - `value`: Native metadata snapshot returned by local I/O.
///
/// # Returns
///
/// Portable kind, length, and timestamp fields supported by the native
/// snapshot.
pub(crate) fn metadata(value: native_files::LocalFileMetadata) -> FileMetadata {
    let mut result = FileMetadata::new(match value.kind() {
        native_files::LocalFileKind::File => FileKind::File,
        native_files::LocalFileKind::Directory => FileKind::Directory,
        native_files::LocalFileKind::Symlink => FileKind::Symlink,
        native_files::LocalFileKind::Other => {
            FileKind::Other("local".to_owned())
        }
    });
    result.len = Some(value.len());
    result.accessed_at = value.accessed_at();
    result.modified_at = value.modified_at();
    result.created_at = value.created_at();
    result
}

/// Converts a native copy outcome to its portable facade representation.
///
/// # Parameters
///
/// - `value`: Completed native copy outcome.
///
/// # Returns
///
/// Portable copy statistics, method, atomicity, durability, and metadata
/// preservation guarantees.
pub(crate) fn copy(value: native_files::LocalCopyOutcome) -> CopyOutcome {
    let stats = value.stats();
    let mut result = CopyOutcome::new(
        CopyStats {
            files: stats.files(),
            directories: stats.directories(),
            bytes: stats.bytes(),
            skipped: stats.skipped(),
            overwritten: stats.overwritten(),
            ..Default::default()
        },
        match value.method() {
            native_files::LocalCopyMethod::StagedFile => CopyMethod::Native,
            native_files::LocalCopyMethod::Recursive => CopyMethod::Native,
        },
        if value.atomic() {
            AchievedAtomicity::Atomic
        } else {
            AchievedAtomicity::NonAtomic
        },
    );
    result = result.with_durable(value.durable());
    result = result.with_metadata(match value.metadata_preservation() {
        native_files::LocalMetadataPreservePolicy::None => {
            MetadataPreservePolicy::None
        }
        native_files::LocalMetadataPreservePolicy::Permissions => {
            MetadataPreservePolicy::Portable
        }
    });
    result
}

/// Converts a completed native rename to a portable atomic rename outcome.
///
/// # Parameters
///
/// - `_value`: Native completion marker; the native API guarantees atomic
///   rename semantics for successful outcomes.
/// - `source`: Logical source path supplied by the caller.
/// - `target`: Logical target path supplied by the caller.
///
/// # Returns
///
/// An atomic-rename outcome retaining both logical paths.
#[inline]
pub(crate) fn rename(
    _value: native_files::LocalRenameOutcome,
    source: &qubit_fs::Path,
    target: &qubit_fs::Path,
) -> RenameOutcome {
    RenameOutcome::new(
        source.clone(),
        target.clone(),
        AchievedAtomicity::Atomic,
        PublicationMethod::AtomicRename,
    )
}

/// Converts native copy failure state to its portable equivalent.
///
/// # Parameters
///
/// - `state`: Native copy publication state.
///
/// # Returns
///
/// The equivalent portable copy failure state.
#[inline]
pub(crate) fn copy_failure_state(
    state: native_files::LocalCopyFailureState,
) -> CopyFailureState {
    match state {
        native_files::LocalCopyFailureState::Unchanged => {
            CopyFailureState::Unchanged
        }
        native_files::LocalCopyFailureState::PartiallyPublished => {
            CopyFailureState::PartiallyPublished
        }
        native_files::LocalCopyFailureState::Published => {
            CopyFailureState::Published
        }
        native_files::LocalCopyFailureState::Indeterminate => {
            CopyFailureState::Indeterminate
        }
    }
}

/// Converts native rename failure state to its portable equivalent.
///
/// # Parameters
///
/// - `state`: Native rename namespace state.
///
/// # Returns
///
/// The equivalent portable rename failure state.
#[inline]
pub(crate) fn rename_failure_state(
    state: native_files::LocalRenameFailureState,
) -> RenameFailureState {
    match state {
        native_files::LocalRenameFailureState::Unchanged => {
            RenameFailureState::Unchanged
        }
        native_files::LocalRenameFailureState::Renamed => {
            RenameFailureState::Renamed
        }
        native_files::LocalRenameFailureState::Indeterminate => {
            RenameFailureState::Indeterminate
        }
    }
}

/// Declines native copy so the facade may select a fallback.
///
/// # Returns
///
/// A `NotApplicable` copy attempt.
#[inline(always)]
pub(crate) fn declined_copy() -> CopyAttempt {
    CopyAttempt::Declined(CopyDeclineReason::NotApplicable)
}
