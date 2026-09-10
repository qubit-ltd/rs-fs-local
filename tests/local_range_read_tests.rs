// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Local ranges retain one opened handle and full-resource metadata.

use std::error::Error;
use std::io::Seek;
use std::io::SeekFrom;

use qubit_fs::Path;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::metadata::FileSystemCapability;
use qubit_fs::metadata::FileSystemCapabilitySupport;
use qubit_fs::read::ReadOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_io::Input;

/// Host and rooted providers enforce the same window on the opened file.
#[test]
fn test_local_range_window_and_metadata() {
    let root = tempfile::tempdir().expect("root");
    let native = root.path().join("窗口.txt");
    std::fs::write(&native, b"0123456789").expect("seed");
    for rooted in [false, true] {
        let (fs, path) = if rooted {
            (
                LocalFileSystems::rooted(root.path(), LocalResourcePolicy::standard()).expect("rooted"),
                Path::parse("/窗口.txt").expect("logical path"),
            )
        } else {
            (
                LocalFileSystems::host(LocalResourcePolicy::standard()).expect("host"),
                Path::parse(native.to_str().expect("UTF-8 path")).expect("host path"),
            )
        };
        assert_eq!(
            fs.properties().capabilities().support(FileSystemCapability::RangeRead),
            FileSystemCapabilitySupport::Conditional
        );
        for (offset, length, expected) in [
            (0, None, b"0123456789".as_slice()),
            (2, Some(3), b"234".as_slice()),
            (8, Some(9), b"89".as_slice()),
            (3, None, b"3456789".as_slice()),
            (0, Some(0), b"".as_slice()),
            (10, None, b"".as_slice()),
            (11, Some(2), b"".as_slice()),
            (u64::MAX, None, b"".as_slice()),
            (u64::MAX, Some(0), b"".as_slice()),
        ] {
            let mut reader = fs
                .open_reader(
                    &path,
                    ReadOptions::default().with_offset(Some(offset)).with_length(length),
                )
                .expect("range");
            assert_eq!(reader.info().metadata().expect("metadata").len(), Some(10));
            let mut bytes = [0; 32];
            let count = reader.read_fully(&mut bytes).expect("read window");
            assert_eq!(&bytes[..count], expected);
            assert_eq!(reader.read(&mut [0; 1]).expect("repeat EOF"), 0);
        }
    }
}

/// Empty ranges still open and validate their requested resource.
#[test]
fn test_local_empty_range_checks_existence_and_path() {
    let root = tempfile::tempdir().expect("root");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::standard()).expect("rooted");
    let error = fs
        .open_reader(
            &Path::parse("/missing").expect("path"),
            ReadOptions::default().with_length(Some(0)),
        )
        .expect_err("missing");
    assert_eq!(error.kind(), FsErrorKind::NotFound);
    assert!(
        fs.open_reader(
            &Path::parse("relative").expect("path"),
            ReadOptions::default().with_length(Some(0))
        )
        .is_err()
    );
}

/// Renaming the pathname after opening does not replace the retained resource.
#[cfg(unix)]
#[test]
fn test_local_range_does_not_reopen_path() {
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("file");
    std::fs::write(&source, b"original").expect("seed");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::standard()).expect("rooted");
    let mut reader = fs
        .open_reader(
            &Path::parse("/file").expect("path"),
            ReadOptions::default().with_offset(Some(1)).with_length(Some(3)),
        )
        .expect("range");
    std::fs::rename(&source, root.path().join("old")).expect("rename");
    std::fs::write(&source, b"replacement").expect("replace");
    let mut bytes = [0; 32];
    let count = reader.read_fully(&mut bytes).expect("read");
    assert_eq!(&bytes[..count], b"rig");
}

/// Native filesystem seek limits retain their original error and context.
#[test]
fn test_local_range_preserves_native_seek_result() {
    let root = tempfile::tempdir().expect("root");
    let native = root.path().join("file");
    std::fs::write(&native, b"payload").expect("seed");
    let offset = i64::MAX as u64;
    let native_result = std::fs::File::open(&native)
        .expect("native reader")
        .seek(SeekFrom::Start(offset));
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::standard()).expect("rooted");
    let path = Path::parse("/file").expect("path");
    let result = fs.open_reader(&path, ReadOptions::default().with_offset(Some(offset)));
    match native_result {
        Err(native_error) => {
            let error = result.expect_err("native seek rejection");
            assert_eq!(error.kind(), FsErrorKind::Io);
            assert_eq!(error.operation(), FsOperation::OpenReader);
            assert_eq!(error.path(), Some(&path));
            assert_eq!(error.provider(), Some(fs.properties().info().provider_id()));
            let source = error
                .source()
                .and_then(|source| source.downcast_ref::<std::io::Error>())
                .expect("original native seek error");
            assert_eq!(source.kind(), native_error.kind());
            assert_eq!(source.raw_os_error(), native_error.raw_os_error());
        }
        Ok(_) => {
            let mut reader = result.expect("native seek supported");
            assert_eq!(reader.read(&mut [0; 1]).expect("EOF"), 0);
        }
    }
}
