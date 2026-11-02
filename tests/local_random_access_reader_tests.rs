// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native random-access sessions retain one opened file object.

#[cfg(windows)]
use std::io::ErrorKind;

use qubit_fs::FileSystem;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::path::Path;
use qubit_fs::read::ReadOptions;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::spi::LocalFileSystemSpi;

struct Fixture {
    _root: tempfile::TempDir,
    file_system: FileSystem,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary root");
        let id = FileSystemId::new("local-random-access-test").expect("valid id");
        let spi = LocalFileSystemSpi::rooted(id, root.path(), LocalResourcePolicy::standard()).expect("rooted SPI");
        let file_system = FileSystem::from_spi(spi).expect("filesystem");
        Self {
            _root: root,
            file_system,
        }
    }
}

#[test]
fn test_session_reads_nonsequential_offsets_from_one_opened_file() {
    let fixture = Fixture::new();
    let native_path = fixture._root.path().join("payload");
    std::fs::write(&native_path, b"abcdef").expect("seed file");
    let path = Path::parse("/payload").expect("logical path");

    let mut reader = fixture
        .file_system
        .open_random_access_reader(&path, ReadOptions::default())
        .expect("open random access session");
    assert_eq!(6, reader.len());
    assert_eq!(&path, reader.info().path());
    assert_eq!(Some(6), reader.info().metadata().and_then(|metadata| metadata.len()));

    let mut tail = [0; 2];
    assert_eq!(0, reader.read_at(u64::MAX, &mut []).expect("empty read does not seek"));
    assert_eq!(2, reader.read_at(4, &mut tail).expect("read tail"));
    assert_eq!(*b"ef", tail);
    let mut head = [0; 2];
    assert_eq!(2, reader.read_at(0, &mut head).expect("read head"));
    assert_eq!(*b"ab", head);
    assert_eq!(0, reader.read_at(6, &mut head).expect("EOF"));
}

#[test]
fn test_open_missing_file_maps_native_error_context() {
    let fixture = Fixture::new();
    let path = Path::parse("/missing").expect("logical path");

    let error = fixture
        .file_system
        .open_random_access_reader(&path, ReadOptions::default())
        .expect_err("missing file must fail to open");

    assert_eq!(FsErrorKind::NotFound, error.kind());
    assert_eq!(FsOperation::OpenReader, error.operation());
    assert_eq!(Some(&path), error.path());
}

#[cfg(unix)]
#[test]
fn test_read_at_seek_error_maps_native_io_context() {
    let fixture = Fixture::new();
    let native_path = fixture._root.path().join("payload");
    std::fs::write(&native_path, b"abcdef").expect("seed file");
    let path = Path::parse("/payload").expect("logical path");
    let mut reader = fixture
        .file_system
        .open_random_access_reader(&path, ReadOptions::default())
        .expect("open random access session");

    let error = reader
        .read_at(u64::MAX, &mut [0])
        .expect_err("unrepresentable seek must fail");

    assert_eq!(FsErrorKind::InvalidOptions, error.kind());
    assert_eq!(FsOperation::Read, error.operation());
    assert_eq!(Some(&path), error.path());
}

#[cfg(unix)]
#[test]
fn test_session_observes_truncation_on_the_retained_handle() {
    let fixture = Fixture::new();
    let native_path = fixture._root.path().join("payload");
    std::fs::write(&native_path, b"abcdef").expect("seed file");
    let path = Path::parse("/payload").expect("logical path");
    let mut reader = fixture
        .file_system
        .open_random_access_reader(&path, ReadOptions::default())
        .expect("open random access session");

    std::fs::write(&native_path, b"xy").expect("truncate same file object");
    let mut bytes = [0; 6];
    assert_eq!(2, reader.read_at(0, &mut bytes).expect("short read after truncation"));
    assert_eq!(*b"xy\0\0\0\0", bytes);
    assert_eq!(6, reader.len(), "the session length is its opening observation");
}

#[cfg(unix)]
#[test]
fn test_open_session_remains_bound_after_native_path_replacement() {
    let fixture = Fixture::new();
    let original = fixture._root.path().join("payload");
    let replacement = fixture._root.path().join("replacement");
    std::fs::write(&original, b"AAAAAA").expect("seed original");
    let path = Path::parse("/payload").expect("logical path");
    let mut opened = fixture
        .file_system
        .open_random_access_reader(&path, ReadOptions::default())
        .expect("open original handle");

    std::fs::write(&replacement, b"BBBBBB").expect("seed replacement");
    std::fs::rename(&replacement, &original).expect("Unix rename replaces an existing file");

    let mut bytes = [0; 6];
    assert_eq!(6, opened.read_at(0, &mut bytes).expect("read retained handle"));
    assert_eq!(*b"AAAAAA", bytes);
    let mut newly_opened = fixture
        .file_system
        .open_random_access_reader(&path, ReadOptions::default())
        .expect("open replacement");
    assert_eq!(6, newly_opened.read_at(0, &mut bytes).expect("read replacement"));
    assert_eq!(*b"BBBBBB", bytes);
}

#[cfg(windows)]
#[test]
fn test_open_handle_prevents_native_path_replacement_on_windows() {
    let fixture = Fixture::new();
    let original = fixture._root.path().join("payload");
    let replacement = fixture._root.path().join("replacement");
    std::fs::write(&original, b"AAAAAA").expect("seed original");
    std::fs::write(&replacement, b"BBBBBB").expect("seed replacement");
    let path = Path::parse("/payload").expect("logical path");
    let mut opened = fixture
        .file_system
        .open_random_access_reader(&path, ReadOptions::default())
        .expect("open original handle");

    let error = std::fs::rename(&replacement, &original).expect_err("Windows sharing mode rejects replacement");
    assert_eq!(
        ErrorKind::PermissionDenied,
        error.kind(),
        "platform: Windows; native replacement is denied"
    );
    let mut bytes = [0; 6];
    assert_eq!(6, opened.read_at(0, &mut bytes).expect("read retained handle"));
    assert_eq!(*b"AAAAAA", bytes);
    let remaining = std::fs::read(replacement).expect("replacement remains available");
    assert_eq!(b"BBBBBB", remaining.as_slice());
}
