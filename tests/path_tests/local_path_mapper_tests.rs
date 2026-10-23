// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for public behavior provided by the local path mapper.

#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use qubit_fs_local::host_path_to_logical;

/// Host conversion preserves non-UTF-8 Unix filename bytes.
#[cfg(unix)]
#[test]
fn test_logical_preserves_absolute_non_utf8_host_component() {
    let mut native_path = PathBuf::from(std::path::MAIN_SEPARATOR_STR);
    native_path.push(OsString::from_vec(vec![b'r', b'a', b'w', 0xff]));

    let logical_path = host_path_to_logical(&native_path).expect("absolute non-UTF-8 host path must convert");

    assert_eq!("/raw%FF", logical_path.as_str());
}
