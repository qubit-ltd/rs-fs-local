// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regression coverage for temporary-resource SPI recovery.

use std::ffi::OsStr;

use qubit_fs::copy::CopyOptions;
use qubit_fs::directory::CreateDirectoryOptions;
use qubit_fs::directory::DeleteOptions;
use qubit_fs::error::FsEffectState;
use qubit_fs::error::FsErrorKind;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::path::Path;
use qubit_fs::temp::PersistCleanupState;
use qubit_fs::temp::PersistFailureState;
use qubit_fs::temp::PersistOptions;
use qubit_fs::temp::TempOptions as TempFileOptions;
use qubit_fs::temp::TempOptions as TempDirectoryOptions;
use qubit_fs::temp::TempResourceState;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;
use qubit_local_files::test_support::install_test_fault;

fn run_in_test_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const TEST_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";
    const TEST_FAULT_CHILD_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT_CHILD";
    if std::env::var_os(TEST_FAULT_ENV).is_some_and(|selected| selected == OsStr::new(fault)) {
        let _fault = install_test_fault(fault).expect("test fault controller should install");
        action();
        return;
    }
    if std::env::var_os(TEST_FAULT_CHILD_ENV).is_some() {
        return;
    }
    let executable = std::env::current_exe().expect("test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(TEST_FAULT_ENV, fault)
        .env(TEST_FAULT_CHILD_ENV, "1")
        .status()
        .expect("test fault child should launch");
    assert!(status.success(), "test fault child should pass");
}

/// A recursive native copy failure exposes both the logical request root and
/// the logical child entry that failed.
#[test]
fn test_copy_failure_maps_recursive_failed_child_paths() {
    const TEST_NAME: &str = "test_copy_failure_maps_recursive_failed_child_paths";
    run_in_test_fault_process(TEST_NAME, "copy-staging-copy-second", || {
        let directory = tempfile::tempdir().expect("copy fixture directory must be created");
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        std::fs::create_dir(&source).expect("copy source must be created");
        std::fs::write(source.join("first"), b"first").expect("first source child must be written");
        std::fs::write(source.join("second"), b"second")
            .expect("second source child must be written");
        let source_logical =
            host_path_to_logical(&source).expect("source path must convert to logical path");
        let target_logical =
            host_path_to_logical(&target).expect("target path must convert to logical path");
        let failed_source_logical = host_path_to_logical(&source.join("second"))
            .expect("failed source path must convert to logical path");
        let failed_target_logical = host_path_to_logical(&target.join("second"))
            .expect("failed target path must convert to logical path");

        let failure = LocalFileSystems::host(LocalResourcePolicy::unbounded())
            .expect("host filesystem must be created")
            .copy(&source_logical, &target_logical, CopyOptions::tree())
            .expect_err("second child fault must fail the copy");

        assert_eq!(Some(&source_logical), failure.error().path());
        assert_eq!(Some(&target_logical), failure.error().target());
        assert_eq!(Some(&failed_source_logical), failure.error().failure_path());
        assert_eq!(
            Some(&failed_target_logical),
            failure.error().failure_target(),
        );
    });
}

/// A confirmed destination conflict retains the native temporary file for
/// retry.
#[test]
fn test_temp_file_persist_conflict_retains_resource_for_retry() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("persist-recovery-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem must be created");
    let target = Path::parse("/published.txt").expect("target path must be valid");

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
    assert_eq!(failure.error().kind(), FsErrorKind::AlreadyExists);
    assert_eq!(
        failure.error().effect_state(),
        Some(FsEffectState::Unchanged),
    );
    assert_eq!(temporary.state(), TempResourceState::Owned);

    file_system
        .delete_file(&target, DeleteOptions::default())
        .expect("conflicting target must be removed");
    let outcome = temporary
        .persist(&target, PersistOptions::default())
        .expect("retained temporary file must be persistable after conflict removal");

    assert_eq!(outcome.target(), &target);
}

/// A confirmed destination conflict retains the native temporary directory for
/// retry.
#[test]
fn test_temp_directory_persist_conflict_retains_resource_for_retry() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id =
        FileSystemId::new("persist-directory-recovery-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem must be created");
    let target = Path::parse("/published-directory").expect("target path must be valid");

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
    assert_eq!(failure.error().kind(), FsErrorKind::AlreadyExists);
    assert_eq!(
        failure.error().effect_state(),
        Some(FsEffectState::Unchanged),
    );
    assert_eq!(temporary.state(), TempResourceState::Owned);

    file_system
        .delete_directory(&target, DeleteOptions::default())
        .expect("conflicting target directory must be removed");
    let outcome = temporary
        .persist(&target, PersistOptions::default())
        .expect("retained temporary directory must be persistable after conflict removal");

    assert_eq!(outcome.target(), &target);
}

/// A temporary directory replaces an empty destination when persistence allows
/// overwriting it.
#[test]
fn test_temp_directory_persist_overwrites_empty_destination() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id =
        FileSystemId::new("persist-directory-overwrite-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem must be created");
    let target = Path::parse("/published-directory").expect("target path must be valid");

    file_system
        .create_directory(&target, CreateDirectoryOptions::default())
        .expect("empty destination directory must be created");
    let mut temporary = file_system
        .create_temp_directory(TempDirectoryOptions::default())
        .expect("temporary directory must be created");

    let outcome = temporary
        .persist(&target, PersistOptions::default().with_overwrite(true))
        .expect("overwrite persistence must replace the empty destination");

    assert_eq!(outcome.target(), &target);
}

/// A temporary file replaces an existing file when persistence allows it, and
/// the published resource rejects every later lifecycle operation.
#[test]
fn test_temp_file_persist_overwrites_and_becomes_terminal() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("persist-file-overwrite-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem must be created");
    let target = Path::parse("/published.txt").expect("target path must be valid");

    file_system
        .write_all(&target, b"existing", WriteOptions::default())
        .expect("destination file must be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");
    std::fs::write(
        root.path()
            .join(temporary.path().as_str().trim_start_matches('/')),
        b"published",
    )
    .expect("temporary file fixture must be written");
    let outcome = temporary
        .persist(&target, PersistOptions::default().with_overwrite(true))
        .expect("overwrite persistence must replace the destination file");

    assert_eq!(outcome.target(), &target);
    assert_eq!(temporary.state(), TempResourceState::Persisted);
    let failure = temporary
        .persist(&target, PersistOptions::default())
        .expect_err("published temporary file must reject persistence");
    assert_eq!(
        PersistFailureState::PublishedSourceReleased,
        failure.state(),
    );
    assert_eq!(FsErrorKind::InvalidState, failure.error().kind());
    assert_eq!(None, failure.error().effect_state());
    let keep_failure = temporary
        .keep()
        .expect_err("published temporary file must reject keep");
    assert_eq!(
        PersistFailureState::PublishedSourceReleased,
        keep_failure.state(),
    );
    assert_eq!(FsErrorKind::InvalidState, keep_failure.error().kind());
    assert_eq!(None, keep_failure.error().effect_state());
    let cleanup_error = temporary
        .cleanup()
        .expect_err("published temporary file must reject cleanup");
    assert_eq!(FsErrorKind::InvalidState, cleanup_error.kind());
    assert_eq!(None, cleanup_error.effect_state());
    drop(temporary);
    assert_eq!(
        b"published",
        std::fs::read(root.path().join("published.txt"))
            .expect("dropping the terminal handle must preserve the target")
            .as_slice(),
    );
}

/// Persistence can create a missing destination parent without changing the
/// requested publication target.
#[test]
fn test_temp_file_persist_creates_missing_parent() {
    let root = tempfile::tempdir().expect("test root must be created");
    let file_system = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem must be created");
    let target = Path::parse("/missing/parent/published.txt").expect("target path must be valid");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");
    std::fs::write(
        root.path()
            .join(temporary.path().as_str().trim_start_matches('/')),
        b"payload",
    )
    .expect("temporary file fixture must be written");

    let outcome = temporary
        .persist(&target, PersistOptions::default().with_create_parent())
        .expect("persistence must create missing parents");

    assert_eq!(&target, outcome.target());
    assert_eq!(TempResourceState::Persisted, temporary.state());
    assert_eq!(
        b"payload",
        std::fs::read(root.path().join("missing/parent/published.txt"))
            .expect("published target must be readable")
            .as_slice(),
    );
}

/// An injected install failure reports indeterminate effects and does not let
/// handle destruction reclaim a pre-existing destination.
#[test]
fn test_temp_file_indeterminate_persist_preserves_existing_target_on_drop() {
    const TEST_NAME: &str =
        "test_temp_file_indeterminate_persist_preserves_existing_target_on_drop";
    run_in_test_fault_process(TEST_NAME, "persist-install-indeterminate", || {
        let root = tempfile::tempdir().expect("test root must be created");
        let parent = host_path_to_logical(root.path()).expect("host parent path must be valid");
        let target = host_path_to_logical(&root.path().join("existing.txt"))
            .expect("host target path must be valid");
        let file_system = LocalFileSystems::host(LocalResourcePolicy::unbounded())
            .expect("host filesystem must be created");
        file_system
            .write_all(&target, b"existing", WriteOptions::default())
            .expect("existing target must be written");
        let mut temporary = file_system
            .create_temp_file(TempFileOptions::default().with_parent(Some(parent)))
            .expect("temporary file must be created");

        let failure = temporary
            .persist(&target, PersistOptions::default())
            .expect_err("injected install failure must be reported");

        assert_eq!(PersistFailureState::Indeterminate, failure.state());
        assert_eq!(FsErrorKind::PermissionDenied, failure.error().kind());
        assert_eq!(
            Some(FsEffectState::Indeterminate),
            failure.error().effect_state(),
        );
        assert_eq!(TempResourceState::Indeterminate, temporary.state());
        drop(temporary);
        assert_eq!(
            b"existing",
            std::fs::read(root.path().join("existing.txt"))
                .expect("drop must preserve the existing target")
                .as_slice(),
        );
    });
}

/// Verifies the local provider preserves residual native sandbox cleanup state.
#[test]
fn test_temp_file_persist_reports_residual_cleanup_state() {
    run_in_test_fault_process(
        "test_temp_file_persist_reports_residual_cleanup_state",
        "temp-file-sandbox-remove",
        || {
            let root = tempfile::tempdir().expect("test root must be created");
            let id = FileSystemId::new("persist-cleanup-state-root")
                .expect("test identity must be valid");
            let file_system =
                LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
                    .expect("rooted filesystem must be created");
            let target = Path::parse("/published.txt").expect("target path must be valid");
            let mut temporary = file_system
                .create_temp_file(TempFileOptions::default())
                .expect("temporary file must be created");

            let outcome = temporary
                .persist(&target, PersistOptions::default())
                .expect("publication should succeed");

            assert_eq!(
                PersistCleanupState::ResidualTemporaryContainer,
                outcome.cleanup_state()
            );
            assert!(file_system.stat(&target).is_ok());
        },
    );
}

/// A non-conflict installation failure leaves the publication result
/// indeterminate and blocks later lifecycle commands.
#[test]
fn test_temp_file_persist_install_failure_is_indeterminate() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id =
        FileSystemId::new("persist-file-indeterminate-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem must be created");
    let target = Path::parse("/directory-target").expect("target path must be valid");

    file_system
        .create_directory(&target, CreateDirectoryOptions::default())
        .expect("incompatible destination directory must be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");
    let failure = temporary
        .persist(&target, PersistOptions::default().with_overwrite(true))
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
    let id = FileSystemId::new("temp-directory-keep-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem must be created");
    let mut temporary = file_system
        .create_temp_directory(TempDirectoryOptions::default())
        .expect("temporary directory must be created");
    let path = temporary.path().clone();

    let outcome = temporary
        .keep()
        .expect("temporary directory must be keepable");

    assert_eq!(temporary.state(), TempResourceState::Kept);
    assert_eq!(outcome.target(), temporary.path());
    assert_ne!(&path, temporary.path());
    file_system
        .stat(temporary.path())
        .expect("kept temporary directory must remain accessible");
}

/// Cleaning a temporary directory removes it and makes its session terminal.
#[test]
fn test_temp_directory_cleanup_removes_directory() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("temp-directory-cleanup-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
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

/// A replacement at the original path is rejected instead of being removed by
/// a cleanup retry.
#[test]
fn test_temp_directory_cleanup_failure_rejects_replacement_path() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("temp-directory-cleanup-retry-root")
        .expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
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
    assert_eq!(error.kind(), FsErrorKind::NotFound);
    assert_eq!(temporary.state(), TempResourceState::CleanupRequired);

    std::fs::write(
        root.path().join(path.as_str().trim_start_matches('/')),
        b"replacement",
    )
    .expect("replacement fixture must be restored");
    let error = temporary
        .cleanup()
        .expect_err("replacement directory must fail identity validation");
    assert_eq!(error.kind(), FsErrorKind::NotDirectory);
    assert_eq!(temporary.state(), TempResourceState::CleanupRequired);
    file_system
        .stat(&path)
        .expect("replacement entry must remain");
}

/// Keeping a temporary file preserves it and makes later lifecycle commands
/// invalid.
#[test]
fn test_temp_file_keep_releases_cleanup_ownership() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("temp-file-keep-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem must be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");
    let path = temporary.path().clone();

    let outcome = temporary.keep().expect("temporary file must be keepable");
    assert_eq!(temporary.state(), TempResourceState::Kept);
    assert_eq!(outcome.target(), temporary.path());
    assert_ne!(&path, temporary.path());
    file_system
        .stat(temporary.path())
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
    let id = FileSystemId::new("temp-file-cleanup-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
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
    assert_eq!(
        failure.state(),
        PersistFailureState::NotPublishedSourceReleased
    );
    assert_eq!(FsErrorKind::InvalidState, failure.error().kind());
    assert_eq!(None, failure.error().effect_state());
    assert_eq!(temporary.state(), TempResourceState::Cleaned);
    let cleanup_error = temporary
        .cleanup()
        .expect_err("cleaned temporary file must reject another cleanup");
    assert_eq!(FsErrorKind::InvalidState, cleanup_error.kind());
    assert_eq!(None, cleanup_error.effect_state());
}

/// A kept temporary directory rejects a later keep request.
#[test]
fn test_temp_directory_keep_makes_lifecycle_terminal() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id =
        FileSystemId::new("temp-directory-terminal-root").expect("test identity must be valid");
    let file_system =
        LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
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
    assert_eq!(error.error().kind(), FsErrorKind::InvalidState);
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
    let file_system = LocalFileSystems::host(LocalResourcePolicy::unbounded())
        .expect("host filesystem must be created");
    let target = Path::parse(
        root.path()
            .join("published.txt")
            .to_str()
            .expect("target path must be valid UTF-8"),
    )
    .expect("host target path must be valid");
    let mut file = file_system
        .create_temp_file(
            TempFileOptions::default()
                .with_parent(Some(parent.clone()))
                .with_prefix("host-file-".to_owned())
                .with_suffix(".tmp".to_owned()),
        )
        .expect("host temporary file must be created");
    let outcome = file
        .persist(&target, PersistOptions::default())
        .expect("host temporary file must persist");
    assert_eq!(outcome.target(), &target);

    let mut directory = file_system
        .create_temp_directory(
            TempDirectoryOptions::default()
                .with_parent(Some(parent))
                .with_prefix("host-directory-".to_owned())
                .with_suffix(".tmp".to_owned()),
        )
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
