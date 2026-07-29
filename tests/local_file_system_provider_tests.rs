// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Registry integration tests for the local `file:` provider.

use qubit_fs::{
    ConnectionUri,
    FileSystemId,
    NonSensitiveMetadata,
    UserMetadata,
};
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

/// Provider configuration rejects every URI and metadata feature outside the
/// local adapter contract.
#[test]
fn test_local_provider_rejects_unsupported_configuration_shapes() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::default())
        .expect("the local provider descriptor must register");
    for config in [
        FileSystemConfig::new(
            ConnectionUri::parse("memory:///data")
                .expect("test URI must parse"),
        ),
        FileSystemConfig::new(
            ConnectionUri::parse("file:///data?cache=true")
                .expect("test URI must parse"),
        ),
        FileSystemConfig::new(
            ConnectionUri::parse("file:///data").expect("test URI must parse"),
        )
        .with_options(NonSensitiveMetadata::from(
            UserMetadata::new()
                .with("mode", "test")
                .expect("test metadata must be valid"),
        )),
    ] {
        assert!(
            registry.resolve_config(&config).is_err(),
            "unsupported provider configuration must be rejected"
        );
    }

    let relative = FileSystemConfig::new(
        ConnectionUri::parse("file:relative/path")
            .expect("relative file URI must parse"),
    );
    assert!(
        registry.resolve_config(&relative).is_err(),
        "relative local file URIs must be rejected"
    );
}

/// A rooted provider resolves accepted file URIs inside its retained native
/// authority.
#[test]
fn test_rooted_local_provider_resolves_file_uri() {
    let root = tempfile::tempdir().expect("provider root must be created");
    let id = FileSystemId::new("provider-rooted-local")
        .expect("test identity must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::rooted(id.clone(), root.path()))
        .expect("the rooted local provider descriptor must register");
    let config = FileSystemConfig::new(
        ConnectionUri::parse("file:///inside-root")
            .expect("test URI must parse"),
    );

    let resolution = registry
        .resolve_config(&config)
        .expect("rooted local provider must resolve file URI");

    assert_eq!(resolution.file_system().properties().info().id(), &id);
}

/// Rooted providers report initialization failure when their retained native
/// authority cannot be opened.
#[test]
fn test_rooted_local_provider_rejects_missing_root() {
    let root = std::env::temp_dir().join(format!(
        "qubit-fs-local-missing-root-{}",
        std::process::id()
    ));
    let id = FileSystemId::new("provider-missing-root")
        .expect("test identity must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::rooted(id, &root))
        .expect("the rooted provider descriptor must register");
    let config = FileSystemConfig::new(
        ConnectionUri::parse("file:///inside-root")
            .expect("test file URI must parse"),
    );

    assert!(
        registry.resolve_config(&config).is_err(),
        "a missing rooted authority must not resolve"
    );
}
