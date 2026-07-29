// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Registry integration tests for the local `file:` provider.

use qubit_fs::ConnectionUri;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_registry::{
    FileSystemConfig,
    FileSystemRegistry,
};

/// A registered host provider resolves an absolute `file:` URI.
#[test]
fn test_local_provider_returns_concrete_resolution() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::new())
        .expect("the local provider descriptor must register");
    let config = FileSystemConfig::new(
        ConnectionUri::parse("file:///tmp/data")
            .expect("the test file URI must parse"),
    );

    let resolution = registry
        .resolve_config(&config)
        .expect("the local provider must resolve a host file URI");

    let _: &qubit_fs::FileSystem = resolution.file_system();
    assert_eq!(resolution.canonical_uri().scheme(), "file");
}

/// Remote URI authorities do not have host-local filesystem authority.
#[test]
fn test_local_provider_rejects_remote_authority() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::new())
        .expect("the local provider descriptor must register");
    let config = FileSystemConfig::new(
        ConnectionUri::parse("file://remote/share")
            .expect("the test URI must parse"),
    );

    assert!(registry.resolve_config(&config).is_err());
}
