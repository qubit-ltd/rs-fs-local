// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regression coverage for local `file:` URI path decoding.

use qubit_fs::{
    ConnectionUri,
    FileSystemId,
};
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_registry::{
    FileSystemConfig,
    FileSystemRegistry,
};

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
        .register(LocalFileSystemProvider::rooted(id, root.path()).expect("rooted provider must open"))
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
        .register(LocalFileSystemProvider::rooted(id, root.path()).expect("rooted provider must open"))
        .expect("the rooted local provider descriptor must register");

    for uri in [
        "file:///parent%2Fchild",
        "file:///parent%5Cchild",
        "file:///name%00",
    ] {
        let error = registry
            .resolve_config(&FileSystemConfig::new(
                ConnectionUri::parse(uri).expect("test URI must parse"),
            ))
            .expect_err("unsafe encoded path component must be rejected");
        let qubit_fs_registry::FileSystemRegistryError::Creation(creation) =
            error
        else {
            panic!("expected provider creation error")
        };
        assert_eq!(
            qubit_fs::FsErrorKind::InvalidPath,
            creation.decisive_attempt().failure().error().kind()
        );
    }
}
