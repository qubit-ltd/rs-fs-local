// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public path conversion behavior tests.

use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::path::Path;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;

/// Rooted conversion rejects relative logical paths before native I/O.
#[test]
fn test_native_rejects_rooted_relative_logical_path() {
    let root = tempfile::tempdir().expect("rooted fixture root must exist");
    let file_system = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted local filesystem must be opened");
    let path = Path::parse("relative").expect("relative logical path text");

    let error = file_system
        .stat(&path)
        .expect_err("rooted conversion requires an absolute logical path");

    assert_eq!(FsErrorKind::InvalidPath, error.kind());
    assert_eq!(FsOperation::ParsePath, error.operation());
    assert_eq!(None, error.path());
}
