// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regression coverage for temporary-resource SPI recovery.

use qubit_fs::{
    CreateDirectoryOptions,
    DeleteOptions,
    FileSystemId,
    FsErrorKind,
    Path,
    PersistFailureState,
    PersistOptions,
    TempDirectoryOptions,
    TempFileOptions,
    TempResourceState,
    WriteOptions,
};
use qubit_fs_local::LocalFileSystems;

/// A confirmed destination conflict retains the native temporary file for
/// retry.
#[test]
fn test_temp_file_persist_conflict_retains_resource_for_retry() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("persist-recovery-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let target =
        Path::parse("/published.txt").expect("target path must be valid");

    file_system
        .write_all(&target, b"existing", WriteOptions::default())
        .expect("conflicting target must be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");

    let failure = temporary
        .persist(&target, PersistOptions::default())
        .expect_err("persist must report the existing destination");
    assert_eq!(failure.state(), PersistFailureState::NotPublished);
    assert_eq!(temporary.state(), TempResourceState::Owned);

    file_system
        .delete_file(&target, DeleteOptions::default())
        .expect("conflicting target must be removed");
    let outcome = temporary
        .persist(&target, PersistOptions::default())
        .expect(
        "retained temporary file must be persistable after conflict removal",
    );

    assert_eq!(outcome.target, target);
}

/// A confirmed destination conflict retains the native temporary directory for
/// retry.
#[test]
fn test_temp_directory_persist_conflict_retains_resource_for_retry() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("persist-directory-recovery-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let target =
        Path::parse("/published-directory").expect("target path must be valid");

    file_system
        .create_directory(&target, CreateDirectoryOptions::default())
        .expect("conflicting target directory must be created");
    let mut temporary = file_system
        .create_temp_directory(TempDirectoryOptions::default())
        .expect("temporary directory must be created");

    let failure = temporary
        .persist(&target, PersistOptions::default())
        .expect_err("persist must report the existing destination");
    assert_eq!(failure.state(), PersistFailureState::NotPublished);
    assert_eq!(temporary.state(), TempResourceState::Owned);

    file_system
        .delete_directory(&target, DeleteOptions::default())
        .expect("conflicting target directory must be removed");
    let outcome = temporary
        .persist(&target, PersistOptions::default())
        .expect("retained temporary directory must be persistable after conflict removal");

    assert_eq!(outcome.target, target);
}

/// A temporary directory replaces an empty destination when persistence allows
/// overwriting it.
#[test]
fn test_temp_directory_persist_overwrites_empty_destination() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("persist-directory-overwrite-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let target =
        Path::parse("/published-directory").expect("target path must be valid");

    file_system
        .create_directory(&target, CreateDirectoryOptions::default())
        .expect("empty destination directory must be created");
    let mut temporary = file_system
        .create_temp_directory(TempDirectoryOptions::default())
        .expect("temporary directory must be created");

    let outcome = temporary
        .persist(
            &target,
            PersistOptions {
                overwrite: true,
                ..PersistOptions::default()
            },
        )
        .expect("overwrite persistence must replace the empty destination");

    assert_eq!(outcome.target, target);
}

/// A temporary file replaces an existing file when persistence allows it, and
/// the published resource rejects every later lifecycle operation.
#[test]
fn test_temp_file_persist_overwrites_and_becomes_terminal() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("persist-file-overwrite-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let target =
        Path::parse("/published.txt").expect("target path must be valid");

    file_system
        .write_all(&target, b"existing", WriteOptions::default())
        .expect("destination file must be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");
    let outcome = temporary
        .persist(
            &target,
            PersistOptions {
                overwrite: true,
                ..PersistOptions::default()
            },
        )
        .expect("overwrite persistence must replace the destination file");

    assert_eq!(outcome.target, target);
    assert_eq!(temporary.state(), TempResourceState::Persisted);
    assert_eq!(
        temporary
            .persist(&target, PersistOptions::default())
            .expect_err("published temporary file must reject persistence")
            .state(),
        PersistFailureState::NotPublished
    );
    assert_eq!(
        temporary
            .keep()
            .expect_err("published temporary file must reject keep")
            .kind(),
        FsErrorKind::InvalidState
    );
    assert_eq!(
        temporary
            .cleanup()
            .expect_err("published temporary file must reject cleanup")
            .kind(),
        FsErrorKind::InvalidState
    );
}

/// A non-conflict installation failure leaves the publication result
/// indeterminate and blocks later lifecycle commands.
#[test]
fn test_temp_file_persist_install_failure_is_indeterminate() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("persist-file-indeterminate-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let target =
        Path::parse("/directory-target").expect("target path must be valid");

    file_system
        .create_directory(&target, CreateDirectoryOptions::default())
        .expect("incompatible destination directory must be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");
    let failure = temporary
        .persist(
            &target,
            PersistOptions {
                overwrite: true,
                ..PersistOptions::default()
            },
        )
        .expect_err("file persistence cannot replace a directory");

    assert_eq!(failure.state(), PersistFailureState::NotPublished);
    assert_eq!(temporary.state(), TempResourceState::Owned);
    temporary
        .cleanup()
        .expect("known-unpublished temporary file should remain recoverable");
}

/// Keeping a temporary directory preserves it and releases automatic cleanup.
#[test]
fn test_temp_directory_keep_preserves_directory() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("temp-directory-keep-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let mut temporary = file_system
        .create_temp_directory(TempDirectoryOptions::default())
        .expect("temporary directory must be created");
    let path = temporary.path().clone();

    temporary
        .keep()
        .expect("temporary directory must be keepable");

    assert_eq!(temporary.state(), TempResourceState::Kept);
    file_system
        .stat(&path)
        .expect("kept temporary directory must remain accessible");
}

/// Cleaning a temporary directory removes it and makes its session terminal.
#[test]
fn test_temp_directory_cleanup_removes_directory() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("temp-directory-cleanup-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let mut temporary = file_system
        .create_temp_directory(TempDirectoryOptions::default())
        .expect("temporary directory must be created");
    let path = temporary.path().clone();

    temporary
        .cleanup()
        .expect("temporary directory must be removable");

    assert_eq!(temporary.state(), TempResourceState::Cleaned);
    let error = file_system
        .stat(&path)
        .expect_err("cleaned temporary directory must be absent");
    assert_eq!(error.kind(), FsErrorKind::NotFound);
}

/// A cleanup failure retains a temporary directory for a later cleanup retry.
#[test]
fn test_temp_directory_cleanup_failure_retains_resource_for_retry() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("temp-directory-cleanup-retry-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let mut temporary = file_system
        .create_temp_directory(TempDirectoryOptions::default())
        .expect("temporary directory must be created");
    let path = temporary.path().clone();

    file_system
        .delete_directory(&path, DeleteOptions::default())
        .expect("temporary directory fixture must be removed");
    let error = temporary
        .cleanup()
        .expect_err("missing temporary directory must make cleanup fail");
    assert_eq!(error.kind(), FsErrorKind::Io);
    assert_eq!(temporary.state(), TempResourceState::CleanupRequired);

    file_system
        .create_directory(&path, CreateDirectoryOptions::default())
        .expect("temporary directory fixture must be restored");
    temporary
        .cleanup()
        .expect("retained temporary directory must support cleanup retry");
    assert_eq!(temporary.state(), TempResourceState::Cleaned);
}

/// Keeping a temporary file preserves it and makes later lifecycle commands
/// invalid.
#[test]
fn test_temp_file_keep_releases_cleanup_ownership() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("temp-file-keep-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");
    let path = temporary.path().clone();

    temporary.keep().expect("temporary file must be keepable");
    assert_eq!(temporary.state(), TempResourceState::Kept);
    file_system
        .stat(&path)
        .expect("kept temporary file must remain accessible");
    let error = temporary
        .cleanup()
        .expect_err("kept temporary file must reject cleanup");
    assert_eq!(error.kind(), FsErrorKind::InvalidState);
}

/// A cleaned temporary file rejects a later persistence request without
/// publishing any target.
#[test]
fn test_temp_file_cleanup_makes_persist_terminal() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("temp-file-cleanup-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");
    temporary
        .cleanup()
        .expect("temporary file must be removable");

    let failure = temporary
        .persist(
            &Path::parse("/terminal.txt").expect("target path must be valid"),
            PersistOptions::default(),
        )
        .expect_err("cleaned temporary file must reject persistence");
    assert_eq!(failure.state(), PersistFailureState::NotPublished);
    assert_eq!(temporary.state(), TempResourceState::Cleaned);
}

/// A kept temporary directory rejects a later keep request.
#[test]
fn test_temp_directory_keep_makes_lifecycle_terminal() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("temp-directory-terminal-root")
        .expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let mut temporary = file_system
        .create_temp_directory(TempDirectoryOptions::default())
        .expect("temporary directory must be created");
    temporary
        .keep()
        .expect("temporary directory must be keepable");

    let error = temporary
        .keep()
        .expect_err("kept temporary directory must reject another keep");
    assert_eq!(error.kind(), FsErrorKind::InvalidState);
}

/// Host temporary resources translate persisted and cleaned paths back into
/// the absolute host namespace.
#[cfg(unix)]
#[test]
fn test_host_temp_resources_persist_and_cleanup() {
    let root = tempfile::tempdir().expect("test root must be created");
    let parent = Path::parse(
        root.path()
            .to_str()
            .expect("test root path must be valid UTF-8"),
    )
    .expect("host parent path must be valid");
    let file_system =
        LocalFileSystems::host().expect("host filesystem must be created");
    let target = Path::parse(
        root.path()
            .join("published.txt")
            .to_str()
            .expect("target path must be valid UTF-8"),
    )
    .expect("host target path must be valid");
    let mut file = file_system
        .create_temp_file(TempFileOptions {
            parent: Some(parent.clone()),
            prefix: "host-file-".to_owned(),
            suffix: ".tmp".to_owned(),
        })
        .expect("host temporary file must be created");
    let outcome = file
        .persist(&target, PersistOptions::default())
        .expect("host temporary file must persist");
    assert_eq!(outcome.target, target);

    let mut directory = file_system
        .create_temp_directory(TempDirectoryOptions {
            parent: Some(parent),
            prefix: "host-directory-".to_owned(),
            suffix: ".tmp".to_owned(),
        })
        .expect("host temporary directory must be created");
    let directory_path = directory.path().clone();
    directory
        .cleanup()
        .expect("host temporary directory must clean up");
    let error = file_system
        .stat(&directory_path)
        .expect_err("cleaned host temporary directory must be absent");
    assert_eq!(error.kind(), FsErrorKind::NotFound);
}
