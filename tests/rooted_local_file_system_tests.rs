// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::{
    FileKind,
    FileSystem,
    FileSystemExt,
    FileSystemId,
    FsErrorKind,
    FsPath,
    WriteOptions,
};
use qubit_fs_local::RootedLocalFileSystem;

/// Opens a rooted filesystem with a stable test identity.
#[cfg(unix)]
fn open_rooted_file_system(
    id: &str,
    path: &std::path::Path,
) -> RootedLocalFileSystem {
    let id =
        FileSystemId::new(id).expect("the test filesystem ID should validate");
    RootedLocalFileSystem::open(id, path).expect("the root should open")
}

/// Verifies rooted writes and reads stay beneath the opened directory.
#[cfg(unix)]
#[test]
fn test_rooted_local_file_system_round_trips_content() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system =
        open_rooted_file_system("rooted-round-trip", directory.path());
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

/// Verifies rooted stat reports the root, directories, and final symbolic
/// links.
#[cfg(unix)]
#[test]
fn test_stat_reports_root_directory_and_final_symbolic_link() {
    use std::os::unix::fs::symlink;

    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    std::fs::create_dir(directory.path().join("nested"))
        .expect("the nested directory should be created");
    std::fs::write(directory.path().join("value.txt"), b"value")
        .expect("the regular file should be created");
    symlink("value.txt", directory.path().join("value-link"))
        .expect("the final symbolic link should be created");
    let file_system = open_rooted_file_system("rooted-stat", directory.path());

    assert_eq!(
        FileKind::Directory,
        file_system
            .stat(&FsPath::root())
            .expect("the root should be statable")
            .kind,
    );
    let directory_path =
        FsPath::parse("/nested").expect("the path should parse");
    assert_eq!(
        FileKind::Directory,
        file_system
            .stat(&directory_path)
            .expect("the directory should be statable")
            .kind,
    );
    let link_path =
        FsPath::parse("/value-link").expect("the path should parse");
    assert_eq!(
        FileKind::Symlink,
        file_system
            .stat(&link_path)
            .expect("the final link should be statable")
            .kind,
    );
}

/// Verifies rooted paths decode canonical percent and non-UTF-8 components.
#[cfg(unix)]
#[test]
fn test_open_reader_decodes_canonical_native_components() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    std::fs::write(directory.path().join("100%ready.txt"), b"percent")
        .expect("the percent file should be created");
    let non_utf8_name = OsString::from_vec(vec![b'f', b'o', 0x80, b'o']);
    std::fs::write(directory.path().join(&non_utf8_name), b"non-utf8")
        .expect("the non-UTF-8 file should be created");
    let file_system = open_rooted_file_system("rooted-codec", directory.path());

    let percent_path =
        FsPath::parse("/100%25ready.txt").expect("the path should parse");
    assert_eq!(
        b"percent",
        file_system
            .read_all(&percent_path, 64)
            .expect("the percent file should be read")
            .as_slice(),
    );
    let non_utf8_path =
        FsPath::parse("/fo%80o").expect("the path should parse");
    assert_eq!(
        b"non-utf8",
        file_system
            .read_all(&non_utf8_path, 64)
            .expect("the non-UTF-8 file should be read")
            .as_slice(),
    );
}

/// Verifies unsupported write metadata is rejected before a rooted file opens.
#[cfg(unix)]
#[test]
fn test_open_writer_rejects_content_metadata() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system =
        open_rooted_file_system("rooted-write-options", directory.path());
    let path = FsPath::parse("/value.txt").expect("the path should parse");
    let options = WriteOptions {
        content_type: Some("text/plain".to_owned()),
        ..WriteOptions::default()
    };

    let error = file_system
        .open_writer(&path, options)
        .expect_err("unsupported content metadata should be rejected");

    assert_eq!(FsErrorKind::InvalidOptions, error.kind());
    assert!(!directory.path().join("value.txt").exists());
}

/// Verifies opened locations distinguish different rooted authorities.
#[cfg(unix)]
#[test]
fn test_open_reader_distinguishes_rooted_filesystem_ids() {
    let first = tempfile::tempdir().expect("the first root should be created");
    let second =
        tempfile::tempdir().expect("the second root should be created");
    std::fs::write(first.path().join("value.txt"), b"first")
        .expect("the first file should be created");
    std::fs::write(second.path().join("value.txt"), b"second")
        .expect("the second file should be created");
    let first_file_system =
        open_rooted_file_system("rooted-first", first.path());
    let second_file_system =
        open_rooted_file_system("rooted-second", second.path());
    let path = FsPath::parse("/value.txt").expect("the path should parse");

    let first_reader = first_file_system
        .open_reader(&path, Default::default())
        .expect("the first file should open");
    let second_reader = second_file_system
        .open_reader(&path, Default::default())
        .expect("the second file should open");

    assert_ne!(
        first_reader.info().location(),
        second_reader.info().location(),
    );
}
