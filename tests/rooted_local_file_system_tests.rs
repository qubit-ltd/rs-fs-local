// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::{
    FileSystemExt,
    FsPath,
};
use qubit_fs_local::RootedLocalFileSystem;

/// Verifies rooted writes and reads stay beneath the opened directory.
#[cfg(unix)]
#[test]
fn test_rooted_local_file_system_round_trips_content() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system = RootedLocalFileSystem::open(directory.path())
        .expect("the root should open");
    let path = FsPath::parse("/value.txt").expect("the path should parse");

    file_system
        .write_all(&path, b"rooted")
        .expect("the rooted write should succeed");
    assert_eq!(
        b"rooted",
        file_system
            .read_all(&path, 32)
            .expect("the rooted read should succeed")
            .as_slice(),
    );
}
