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

use qubit_fs::{
    AchievedAtomicity,
    CopyMethod,
    CopyOutcome,
    CopyStats,
    FileKind,
    FileMetadata,
    PublicationMethod,
    RenameOutcome,
};
use qubit_local_files as native_files;

pub(crate) enum LocalOutcomeMapper {}
impl LocalOutcomeMapper {
    pub(crate) fn metadata(
        value: native_files::LocalFileMetadata,
    ) -> FileMetadata {
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
        result
    }
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
}
