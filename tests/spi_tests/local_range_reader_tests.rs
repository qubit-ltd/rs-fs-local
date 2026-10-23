// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local ranges retain one opened handle and full-resource metadata.

use std::error::Error;
use std::io::Seek;
use std::io::SeekFrom;

use qubit_fs::Path;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::read::ReadOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_io::Input;

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

/// Native seek failures retain context, and subsequent reads match the native
/// handle.
#[test]
fn test_local_range_preserves_native_seek_result() {
    let root = tempfile::tempdir().expect("root");
    let native = root.path().join("file");
    std::fs::write(&native, b"payload").expect("seed");
    let offset = i64::MAX as u64;
    let mut native_reader = std::fs::File::open(&native).expect("native reader");
    let native_result = native_reader.seek(SeekFrom::Start(offset));
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
        Ok(position) => {
            assert_eq!(position, offset, "native seek must reach the requested offset");
            let mut reader = result.expect("native seek supported");
            // A successful seek need not make the next native read valid at
            // extreme offsets. Preserve the read result as well as seek errors.
            let mut native_bytes = [0; 1];
            let mut bytes = [0; 1];
            let native_read = Input::read(&mut native_reader, &mut native_bytes);
            let read = reader.read(&mut bytes);
            match native_read {
                Ok(native_count) => {
                    let count = read.expect("native read supported");
                    assert_eq!(count, native_count, "range read must preserve the native byte count");
                    assert_eq!(&bytes[..count], &native_bytes[..native_count]);
                }
                Err(native_error) => {
                    let error = read.expect_err("native read rejection must propagate");
                    assert_eq!(error.kind(), native_error.kind());
                    assert_eq!(error.raw_os_error(), native_error.raw_os_error());
                }
            }
        }
    }
}
