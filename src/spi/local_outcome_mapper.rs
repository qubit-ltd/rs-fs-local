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

use qubit_fs::copy::CopyFailureState;
use qubit_fs::copy::CopyMethod;
use qubit_fs::copy::CopyOutcome;
use qubit_fs::copy::CopyStats;
use qubit_fs::copy::MetadataPreservePolicy;
use qubit_fs::metadata::AchievedAtomicity;
use qubit_fs::metadata::FileKind;
use qubit_fs::metadata::FileMetadata;
use qubit_fs::metadata::PublicationMethod;
use qubit_fs::path::Path;
use qubit_fs::rename::RenameFailureState;
use qubit_fs::rename::RenameOutcome;
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
pub(crate) fn metadata(
    value: native_files::outcome::LocalFileMetadata,
) -> FileMetadata {
    let kind = match value.kind() {
        native_files::outcome::LocalFileKind::File => FileKind::File,
        native_files::outcome::LocalFileKind::Directory => FileKind::Directory,
        native_files::outcome::LocalFileKind::Symlink => FileKind::Symlink,
        native_files::outcome::LocalFileKind::Fifo => {
            FileKind::Other("local-fifo".to_owned())
        }
        native_files::outcome::LocalFileKind::Socket => {
            FileKind::Other("local-socket".to_owned())
        }
        native_files::outcome::LocalFileKind::BlockDevice => {
            FileKind::Other("local-block-device".to_owned())
        }
        native_files::outcome::LocalFileKind::CharDevice => {
            FileKind::Other("local-char-device".to_owned())
        }
        native_files::outcome::LocalFileKind::Other => {
            FileKind::Other("local".to_owned())
        }
        _ => FileKind::Other("local".to_owned()),
    };
    FileMetadata::new(kind)
        .with_len(Some(value.len()))
        .with_accessed_at(value.accessed_at())
        .with_modified_at(value.modified_at())
        .with_created_at(value.created_at())
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
pub(crate) fn copy(
    value: native_files::outcome::LocalCopyOutcome,
) -> CopyOutcome {
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
            native_files::outcome::LocalCopyMethod::StagedFile => {
                CopyMethod::Native
            }
            native_files::outcome::LocalCopyMethod::Recursive => {
                CopyMethod::Native
            }
            _ => CopyMethod::Native,
        },
        if value.atomic() {
            AchievedAtomicity::Atomic
        } else {
            AchievedAtomicity::NonAtomic
        },
    );
    result = result.with_durable(value.durable());
    result = result.with_metadata(match value.metadata_preservation() {
        native_files::options::LocalMetadataPreservePolicy::None => {
            MetadataPreservePolicy::None
        }
        native_files::options::LocalMetadataPreservePolicy::Permissions => {
            MetadataPreservePolicy::Portable
        }
    });
    result
}

/// Converts a completed native rename to a portable atomic rename outcome.
///
/// # Parameters
///
/// - `value`: Native completion marker containing atomicity and durability
///   facts from the completed operation.
/// - `source`: Logical source path supplied by the caller.
/// - `target`: Logical target path supplied by the caller.
///
/// # Returns
///
/// An atomic-rename outcome retaining both logical paths.
#[inline]
pub(crate) fn rename(
    value: native_files::outcome::LocalRenameOutcome,
    source: &Path,
    target: &Path,
) -> RenameOutcome {
    RenameOutcome::new(
        source.clone(),
        target.clone(),
        AchievedAtomicity::Atomic,
        PublicationMethod::AtomicRename,
    )
    .with_durable(value.durable())
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
    state: native_files::outcome::LocalCopyFailureState,
) -> CopyFailureState {
    match state {
        native_files::outcome::LocalCopyFailureState::Unchanged => {
            CopyFailureState::Unchanged
        }
        native_files::outcome::LocalCopyFailureState::PartiallyPublished => {
            CopyFailureState::PartiallyPublished
        }
        native_files::outcome::LocalCopyFailureState::Published => {
            CopyFailureState::Published
        }
        native_files::outcome::LocalCopyFailureState::Indeterminate => {
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
    state: native_files::outcome::LocalRenameFailureState,
) -> RenameFailureState {
    match state {
        native_files::outcome::LocalRenameFailureState::Unchanged => {
            RenameFailureState::Unchanged
        }
        native_files::outcome::LocalRenameFailureState::Renamed => {
            RenameFailureState::Renamed
        }
        native_files::outcome::LocalRenameFailureState::Indeterminate => {
            RenameFailureState::Indeterminate
        }
    }
}
