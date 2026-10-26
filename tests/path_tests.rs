// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public path conversion behavior tests.

use qubit_fs::path::Path;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;

/// Rooted relative logical paths are resolved from the virtual PWD.
#[test]
fn test_native_resolves_rooted_relative_logical_path_from_virtual_pwd() {
    let root = tempfile::tempdir().expect("rooted fixture root must exist");
    std::fs::write(root.path().join("relative"), b"rooted relative path")
        .expect("rooted relative target must exist");
    let file_system = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted local filesystem must be opened");
    let path = Path::parse("relative").expect("relative logical path text");

    file_system
        .stat(&path)
        .expect("rooted relative path must resolve from the virtual PWD");
}
