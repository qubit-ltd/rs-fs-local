// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regression coverage for local `file:` URI path decoding.

use qubit_fs::ConnectionUri;
use qubit_fs::FileSystemId;
use qubit_fs::FsErrorKind;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_registry::FileSystemConfig;
use qubit_fs_registry::FileSystemRegistry;
use qubit_fs_registry::FileSystemRegistryError;

/// An encoded literal percent is converted to local canonical path text.
#[test]
fn test_rooted_provider_decodes_literal_percent_path_segment() {
    let root = tempfile::tempdir().expect("provider root must be created");
    std::fs::write(root.path().join("progress%100.txt"), b"payload")
        .expect("percent-path fixture must be written");
    let id = FileSystemId::new("provider-percent-path-root")
        .expect("test identity must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted(id, root.path())
                .expect("rooted provider must open"),
        )
        .expect("the rooted local provider descriptor must register");
    let resolution = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///progress%25100.txt")
                .expect("test URI must parse"),
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
    let id = FileSystemId::new("provider-unsafe-path-root")
        .expect("test identity must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted(id, root.path())
                .expect("rooted provider must open"),
        )
        .expect("the rooted local provider descriptor must register");

    #[cfg(not(windows))]
    let uris = ["file:///parent%2Fchild", "file:///name%00"].as_slice();
    #[cfg(windows)]
    let uris = [
        "file:///parent%2Fchild",
        "file:///parent%5Cchild",
        "file:///name%00",
    ]
    .as_slice();
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
    std::fs::write(root.path().join("parent\\child"), b"payload")
        .expect("backslash-path fixture must be written");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted(
                FileSystemId::new("provider-backslash-path-root")
                    .expect("test identity must be valid"),
                root.path(),
            )
            .expect("rooted provider must open"),
        )
        .expect("the rooted local provider descriptor must register");

    let resolution = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///parent%5Cchild")
                .expect("test URI must parse"),
        ))
        .expect("encoded Unix backslash must resolve");

    assert_eq!(
        "file:///parent%5Cchild",
        resolution.canonical_uri().as_str(),
    );
    resolution
        .file_system()
        .stat(resolution.path())
        .expect("decoded backslash path must access the native fixture");
}
