// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::directory::ListOptions;
use qubit_fs::error::FsErrorKind;
use qubit_fs::metadata::FileSystemLimit;
use qubit_fs::path::Path;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
#[cfg(unix)]
use qubit_fs_local::host_path_to_logical;

/// Escaped canonical components may be longer than their native spelling.
#[test]
fn test_rooted_paths_accept_expanded_percent_components() {
    let root = tempfile::tempdir().expect("fixture root");
    let native_name = "%".repeat(100);
    std::fs::write(root.path().join(&native_name), b"before").expect("native filename must be valid");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted facade");
    let path = Path::parse(&format!("/{}", "%25".repeat(100))).expect("canonical path");
    assert_eq!(Some(6), fs.stat(&path).expect("stat escaped name").len());
    assert_eq!(
        b"before",
        fs.read_all(&path, Default::default(), 32)
            .expect("read escaped name")
            .as_slice()
    );
    fs.write_all(&path, b"after", WriteOptions::default())
        .expect("replace escaped name");
    assert_eq!(
        b"after",
        std::fs::read(root.path().join(&native_name))
            .expect("observe native target")
            .as_slice()
    );
    let mut stream = fs.list(&Path::root(), ListOptions::default()).expect("open list");
    assert_eq!(path, stream.next_entry().expect("valid entry").expect("entry").path);
    assert!(stream.next_entry().expect("end of list").is_none());
}

/// Native byte limits are not limits on canonical logical UTF-8 text.
#[test]
fn test_local_properties_do_not_claim_native_limits_are_text_limits() {
    let root = tempfile::tempdir().expect("fixture root");
    let host = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host");
    let rooted = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    for fs in [host, rooted] {
        assert_eq!(FileSystemLimit::Unknown, fs.properties().limits().max_path_text_bytes());
        assert_eq!(
            FileSystemLimit::Unknown,
            fs.properties().limits().max_component_text_bytes()
        );
    }
}

/// Unix raw filenames remain addressable after canonical percent encoding.
#[cfg(unix)]
#[test]
fn test_rooted_paths_preserve_long_non_utf8_components() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let root = tempfile::tempdir().expect("fixture root");
    let name = OsString::from_vec(vec![0xff; 100]);
    std::fs::write(root.path().join(name), b"raw").expect("native bytes");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    let path = Path::parse(&format!("/{}", "%FF".repeat(100))).expect("logical bytes");
    assert_eq!(
        b"raw",
        fs.read_all(&path, Default::default(), 8)
            .expect("read raw name")
            .as_slice()
    );
    let mut stream = fs.list(&Path::root(), ListOptions::default()).expect("list");
    assert_eq!(path, stream.next_entry().expect("valid raw entry").expect("entry").path);
    assert!(stream.next_entry().expect("end").is_none());
}

/// Absolute host paths preserve non-UTF-8 Unix components losslessly.
#[cfg(unix)]
#[test]
fn test_absolute_host_paths_preserve_non_utf8_components() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = tempfile::tempdir().expect("fixture root");
    let name = OsString::from_vec(vec![b'r', b'a', b'w', 0xff]);
    let native = directory.path().join(name);
    std::fs::write(&native, b"host").expect("native host fixture");
    let logical = host_path_to_logical(&native).expect("absolute host path must convert without lossy text");
    let fs = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host");

    assert!(logical.as_str().ends_with("/raw%FF"));
    assert_eq!(
        b"host",
        fs.read_all(&logical, Default::default(), 8)
            .expect("converted host path must remain addressable")
            .as_slice(),
    );
}

/// A long escaped logical path can still target a native path under its OS cap.
#[cfg(target_os = "linux")]
#[test]
fn test_rooted_paths_accept_expanded_total_path_text() {
    let root = tempfile::tempdir().expect("fixture root");
    let mut native = root.path().to_path_buf();
    let mut components = Vec::new();
    for _ in 0..24 {
        native.push("%".repeat(60));
        components.push("%25".repeat(60));
    }
    std::fs::create_dir_all(&native).expect("native total path fits");
    std::fs::write(native.join("item"), b"deep").expect("deep fixture");
    components.push("item".to_owned());
    let path = Path::parse(&format!("/{}", components.join("/"))).expect("logical path");
    assert!(path.as_str().len() > 4096);
    assert!(components.iter().all(|component| component.len() <= 255));
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    assert_eq!(
        b"deep",
        fs.read_all(&path, Default::default(), 8)
            .expect("read expanded total path")
            .as_slice()
    );
}

/// Unknown text limits still leave malformed and native-invalid paths rejected.
#[test]
fn test_unknown_text_limits_do_not_accept_invalid_native_components() {
    let root = tempfile::tempdir().expect("fixture root");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    let invalid = Path::parse("/bad%00name").expect("logical spelling");
    assert!(fs.stat(&invalid).is_err());
    let oversized =
        Path::parse(&format!("/{}", "x".repeat(65536))).expect("logical text has no provider-native size semantics");
    assert!(fs.stat(&oversized).is_err());
    assert_eq!(0, std::fs::read_dir(root.path()).expect("unchanged root").count());
}

/// Relative logical paths fail validation before a rooted target is created.
#[test]
fn test_rooted_relative_logical_path_fails_before_io() {
    let root = tempfile::tempdir().expect("fixture root");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    let relative = Path::parse("relative-target").expect("relative path text");

    let error = fs
        .write_all(&relative, b"payload", WriteOptions::default())
        .expect_err("rooted filesystem requires absolute logical paths");

    assert_eq!(FsErrorKind::InvalidPath, error.error().kind());
    assert!(!root.path().join("relative-target").exists());
}

/// Encoded dot components fail conversion before filesystem mutation.
#[test]
fn test_rooted_dot_components_fail_before_io() {
    let root = tempfile::tempdir().expect("fixture root");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");

    for text in ["/%2E/target", "/%2E%2E/target"] {
        let path = Path::parse(text).expect("escaped dot path text");
        let error = fs
            .write_all(&path, b"payload", WriteOptions::default())
            .expect_err("dot traversal components must be rejected");
        assert_eq!(FsErrorKind::InvalidPath, error.error().kind(), "path: {text}",);
        assert_eq!(Some(&path), error.error().path(), "path: {text}");
    }
    assert!(!root.path().join("target").exists());
}

/// Encoded NUL fails conversion before filesystem mutation.
#[test]
fn test_rooted_nul_component_fails_before_io() {
    let root = tempfile::tempdir().expect("fixture root");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    let path = Path::parse("/invalid%00target").expect("escaped NUL text");

    let error = fs
        .write_all(&path, b"payload", WriteOptions::default())
        .expect_err("NUL component must be rejected");

    assert_eq!(FsErrorKind::InvalidPath, error.error().kind());
    assert_eq!(Some(&path), error.error().path());
    assert!(!root.path().join("invalid\0target").exists());
    assert_eq!(0, std::fs::read_dir(root.path()).expect("unchanged root").count(),);
}
