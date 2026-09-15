// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_fs::directory::DeleteOptions;
use qubit_fs::error::FsErrorKind;
use qubit_fs::path::Path;
use qubit_fs::temp::TempOptions;
use qubit_fs::temp::TempResourceState;
use qubit_fs_local::LocalCopyResourceLimits;
use qubit_fs_local::LocalDeleteResourceLimits;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalListResourceLimits;
use qubit_fs_local::LocalResourcePolicy;

fn policy_with_zero_delete_entries() -> LocalResourcePolicy {
    LocalResourcePolicy::bounded(
        LocalListResourceLimits::new(8, 64, 4096, 4, Duration::from_secs(60)).expect("list"),
        LocalCopyResourceLimits::new(8, 64, 4096, 4, Duration::from_secs(60)).expect("copy"),
        LocalDeleteResourceLimits::new(8, 0, 4096, Duration::from_secs(60)),
    )
}

#[test]
fn test_ordinary_delete_is_bounded_but_temp_cleanup_is_independent() {
    let root = tempfile::tempdir().expect("root");
    let fs =
        LocalFileSystems::rooted(root.path(), policy_with_zero_delete_entries()).expect("rooted");
    std::fs::create_dir(root.path().join("ordinary")).expect("directory");
    std::fs::write(root.path().join("ordinary/child"), b"x").expect("child");
    let error = fs
        .delete_directory(
            &Path::parse("/ordinary").expect("path"),
            DeleteOptions::default().with_recursive(true),
        )
        .expect_err("ordinary delete cap");
    assert_eq!(FsErrorKind::ResourceLimitExceeded, error.kind());
    assert!(root.path().join("ordinary/child").exists());

    let mut temporary = fs
        .create_temp_directory(TempOptions::default())
        .expect("temp");
    let temporary_path = temporary.path().clone();
    let child = Path::parse(&format!("{}/child", temporary_path.as_str())).expect("child path");
    fs.write_all(&child, b"x", Default::default())
        .expect("populate temp");
    temporary
        .cleanup()
        .expect("cleanup does not inherit ordinary deletion cap");
    assert_eq!(TempResourceState::Cleaned, temporary.state());
    assert!(!fs.exists(&temporary_path).expect("cleanup observation"));
}

#[test]
fn test_temp_drop_keeps_existing_best_effort_cleanup_scope() {
    let root = tempfile::tempdir().expect("root");
    let fs =
        LocalFileSystems::rooted(root.path(), policy_with_zero_delete_entries()).expect("rooted");
    let temporary = fs
        .create_temp_directory(TempOptions::default())
        .expect("temp");
    let path = temporary.path().clone();
    let child = Path::parse(&format!("{}/child", path.as_str())).expect("child path");
    fs.write_all(&child, b"x", Default::default())
        .expect("populate temp");
    drop(temporary);
    assert!(!fs.exists(&path).expect("normal Drop cleanup"));
}
