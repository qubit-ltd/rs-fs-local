// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regression coverage for local `file:` URI path decoding.

use qubit_fs::error::FsErrorKind;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::path::ConnectionUri;
#[cfg(windows)]
use qubit_fs::read::ReadOptions;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_local::LocalResourcePolicy;
#[cfg(windows)]
use qubit_fs_local::host_path_to_logical;
use qubit_fs_registry::FileSystemConfig;
use qubit_fs_registry::FileSystemRegistry;
use qubit_fs_registry::FileSystemRegistryError;

/// An encoded literal percent is converted to local canonical path text.
#[test]
fn test_rooted_provider_decodes_literal_percent_path_segment() {
    let root = tempfile::tempdir().expect("provider root must be created");
    std::fs::write(root.path().join("progress%100.txt"), b"payload").expect("percent-path fixture must be written");
    let id = FileSystemId::new("provider-percent-path-root").expect("test identity must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted(id, root.path(), LocalResourcePolicy::unbounded())
                .expect("rooted provider must open"),
        )
        .expect("the rooted local provider descriptor must register");
    let resolution = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///progress%25100.txt").expect("test URI must parse"),
        ))
        .expect("encoded local file URI must resolve");

    resolution
        .file_system()
        .stat(resolution.path())
        .expect("decoded percent path must access the native fixture");
}

/// Encoded separators and NUL bytes must not alter the logical URI hierarchy.
#[test]
fn test_rooted_provider_rejects_unsafe_encoded_path_components() {
    let root = tempfile::tempdir().expect("provider root must be created");
    let id = FileSystemId::new("provider-unsafe-path-root").expect("test identity must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted(id, root.path(), LocalResourcePolicy::unbounded())
                .expect("rooted provider must open"),
        )
        .expect("the rooted local provider descriptor must register");

    #[cfg(not(windows))]
    let uris = ["file:///parent%2Fchild", "file:///name%00"].as_slice();
    #[cfg(windows)]
    let uris = ["file:///parent%2Fchild", "file:///parent%5Cchild", "file:///name%00"].as_slice();
    for uri in uris {
        let error = registry
            .resolve_config(&FileSystemConfig::new(
                ConnectionUri::parse(uri).expect("test URI must parse"),
            ))
            .expect_err("unsafe encoded path component must be rejected");
        let FileSystemRegistryError::Creation(creation) = error else {
            panic!("expected provider creation error")
        };
        assert_eq!(
            FsErrorKind::InvalidPath,
            creation.decisive_attempt().failure().error().kind()
        );
    }
}

/// Unix treats backslash as an ordinary filename byte and preserves it in the
/// canonical URI spelling.
#[cfg(unix)]
#[test]
fn test_rooted_provider_round_trips_encoded_unix_backslash() {
    let root = tempfile::tempdir().expect("provider root must be created");
    std::fs::write(root.path().join("parent\\child"), b"payload").expect("backslash-path fixture must be written");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted(
                FileSystemId::new("provider-backslash-path-root").expect("test identity must be valid"),
                root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("rooted provider must open"),
        )
        .expect("the rooted local provider descriptor must register");

    let resolution = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///parent%5Cchild").expect("test URI must parse"),
        ))
        .expect("encoded Unix backslash must resolve");

    assert_eq!("file:///parent%5Cchild", resolution.canonical_uri().as_str(),);
    resolution
        .file_system()
        .stat(resolution.path())
        .expect("decoded backslash path must access the native fixture");
}

#[cfg(windows)]
#[test]
fn test_windows_host_registry_round_trips_native_file() {
    let root = tempfile::tempdir().expect("host fixture directory must be created");
    let native = root.path().join("report %中.txt");
    let payload = b"windows host URI round trip";
    std::fs::write(&native, payload).expect("seed host file");

    let expected_path = host_path_to_logical(&native).expect("logical host path");
    assert!(expected_path.as_str().ends_with("/report %25中.txt"));
    let parent = host_path_to_logical(root.path()).expect("logical host parent");
    let encoded_parent = encode_host_uri_parent(&parent);
    let uri_text = format!("file://{encoded_parent}/report%20%25%E4%B8%AD.txt");

    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(LocalResourcePolicy::standard()))
        .expect("host provider must register");

    let first = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse(&uri_text).expect("host URI"),
        ))
        .expect("resolve host URI");
    assert_eq!(first.path(), &expected_path);
    assert_eq!(
        first
            .file_system()
            .read_all(first.path(), ReadOptions::default(), payload.len())
            .expect("read first resolution"),
        payload,
    );

    let replay = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse(first.canonical_uri().as_str()).expect("canonical URI"),
        ))
        .expect("replay canonical URI");
    assert_eq!(replay.path(), first.path());
    assert_eq!(replay.canonical_uri(), first.canonical_uri());
    assert_eq!(
        replay.file_system().properties().info().id(),
        first.file_system().properties().info().id(),
    );
    assert_eq!(
        replay
            .file_system()
            .read_all(replay.path(), ReadOptions::default(), payload.len())
            .expect("read replay"),
        payload,
    );
}

#[cfg(windows)]
fn encode_host_uri_parent(path: &qubit_fs::path::Path) -> String {
    let mut result = String::new();
    for byte in path.as_str().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'%' | b'-' | b'.' | b'_' | b'~') {
            result.push(char::from(byte));
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result.trim_end_matches('/').to_owned()
}
