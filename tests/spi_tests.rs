// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Integration tests for the filesystem SPI.

use qubit_fs::Path;
use qubit_fs::metadata::FileSystemCapability;
use qubit_fs::metadata::FileSystemCapabilitySupport;
use qubit_fs::read::ReadOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;
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
                host_path_to_logical(&native).expect("host path"),
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
