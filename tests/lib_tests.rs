// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::{
    FileKind,
    FileSystem,
    FsPath,
};
use qubit_fs_local::LocalFileSystem;

/// Confirms the synchronous local filesystem is part of the crate's public API.
#[test]
fn test_local_file_system_is_exported() {
    let _ = std::any::TypeId::of::<LocalFileSystem>();
}

/// Confirms host-wide metadata is mapped from the native local filesystem.
#[test]
fn test_host_stat_maps_regular_file_metadata() {
    let temporary_directory = tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    std::fs::write(&file_path, b"payload").expect("write test file");
    let fs = LocalFileSystem::host();
    let path = FsPath::parse(file_path.to_string_lossy().as_ref()).expect("parse filesystem path");

    let metadata = fs.stat(&path).expect("stat test file");

    assert_eq!(metadata.kind, FileKind::File);
    assert_eq!(metadata.len, Some(7));
}
