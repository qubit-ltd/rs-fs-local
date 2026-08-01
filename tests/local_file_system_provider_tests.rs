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
    FsErrorKind,
    NonSensitiveMetadata,
    Path,
    UserMetadata,
};
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_registry::{
    FileSystemConfig,
    FileSystemRegistry,
    FileSystemRegistryError,
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

/// Equivalent absolute `file:` spellings resolve to one canonical URI.
#[test]
fn test_local_provider_canonicalizes_file_uri_path() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::new())
        .expect("the local provider descriptor must register");

    let single_slash = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:/tmp/data")
                .expect("single-slash URI must parse"),
        ))
        .expect("single-slash file URI must resolve");
    let triple_slash = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///tmp/data")
                .expect("triple-slash URI must parse"),
        ))
        .expect("triple-slash file URI must resolve");

    assert_eq!(single_slash.canonical_uri(), triple_slash.canonical_uri());
    assert_eq!(single_slash.canonical_uri().as_str(), "file:///tmp/data");
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

    let error = registry
        .resolve_config(&config)
        .expect_err("remote file authority must be rejected");
    let FileSystemRegistryError::Creation(creation) = error else {
        panic!("expected provider creation error")
    };
    assert_eq!(
        creation.decisive_attempt().failure().error().kind(),
        FsErrorKind::InvalidOptions
    );
}

/// Provider configuration rejects every URI and metadata feature outside the
/// local adapter contract.
#[test]
fn test_local_provider_rejects_unsupported_configuration_shapes() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::default())
        .expect("the local provider descriptor must register");
    let unsupported_scheme = FileSystemConfig::new(
        ConnectionUri::parse("memory:///data").expect("test URI must parse"),
    );
    assert!(matches!(
        registry.resolve_config(&unsupported_scheme),
        Err(FileSystemRegistryError::Resolution(_))
    ));

    for config in [
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
        let error = registry
            .resolve_config(&config)
            .expect_err("unsupported provider configuration must be rejected");
        let FileSystemRegistryError::Creation(creation) = error else {
            panic!("expected provider creation error")
        };
        assert_eq!(
            creation.decisive_attempt().failure().error().kind(),
            FsErrorKind::InvalidOptions
        );
    }

    let relative = FileSystemConfig::new(
        ConnectionUri::parse("file:relative/path")
            .expect("relative file URI must parse"),
    );
    let error = registry
        .resolve_config(&relative)
        .expect_err("relative local file URIs must be rejected");
    let FileSystemRegistryError::Creation(creation) = error else {
        panic!("expected provider creation error")
    };
    assert_eq!(
        creation.decisive_attempt().failure().error().kind(),
        FsErrorKind::InvalidPath
    );
}

/// Embedded secrets are unsupported by the local provider and must return a
/// provider-creation error instead of panicking while decoding the URI.
#[test]
fn test_local_provider_rejects_embedded_secrets_without_panicking() {
    for text in [
        "file://user:password@localhost/tmp/data",
        "file:///tmp/data?token=secret",
    ] {
        let registry = FileSystemRegistry::default();
        registry
            .register(LocalFileSystemProvider::new())
            .expect("the local provider descriptor must register");
        let config = FileSystemConfig::new(
            ConnectionUri::parse(text).expect("test connection URI must parse"),
        );

        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                registry.resolve_config(&config)
            }));

        let result = outcome.expect("embedded secrets must not panic");
        assert!(matches!(result, Err(FileSystemRegistryError::Creation(_))));
    }
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
        .register(
            LocalFileSystemProvider::rooted(id.clone(), root.path())
                .expect("rooted provider must open"),
        )
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

/// A rooted provider retains the authority opened at construction even when
/// the diagnostic root pathname is later replaced.
#[test]
fn test_rooted_local_provider_pins_opened_authority() {
    let parent = tempfile::tempdir().expect("provider parent must be created");
    let root = parent.path().join("root");
    std::fs::create_dir(&root).expect("provider root must be created");
    std::fs::write(root.join("value"), b"original")
        .expect("fixture must be written");
    let id = FileSystemId::new("provider-pinned-root")
        .expect("test identity must be valid");
    let provider = LocalFileSystemProvider::rooted(id, &root)
        .expect("rooted provider must open");
    std::fs::rename(&root, parent.path().join("old-root"))
        .expect("opened root path must be replaceable");
    std::fs::create_dir(&root).expect("replacement root must be created");
    std::fs::write(root.join("value"), b"replacement")
        .expect("replacement fixture must be written");

    let registry = FileSystemRegistry::default();
    registry.register(provider).expect("provider must register");
    let resolution = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///value").expect("test URI must parse"),
        ))
        .expect("provider must resolve pinned authority");
    assert_eq!(
        b"original".to_vec(),
        resolution
            .file_system()
            .read_all(resolution.path(), Default::default(), 1024)
            .expect("pinned root must retain original entry")
    );
}

/// A rooted provider decodes percent-encoded file URI path segments before
/// accessing its retained native authority.
#[test]
fn test_rooted_local_provider_decodes_percent_encoded_path_segments() {
    let root = tempfile::tempdir().expect("provider root must be created");
    std::fs::write(root.path().join("report final.txt"), b"payload")
        .expect("encoded-path fixture must be written");
    std::fs::write(root.path().join("café.txt"), b"payload")
        .expect("UTF-8 encoded-path fixture must be written");
    let id = FileSystemId::new("provider-encoded-path-root")
        .expect("test identity must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted(id, root.path())
                .expect("rooted provider must open"),
        )
        .expect("the rooted local provider descriptor must register");

    for uri in ["file:///report%20final.txt", "file:///caf%C3%A9.txt"] {
        let resolution = registry
            .resolve_config(&FileSystemConfig::new(
                ConnectionUri::parse(uri).expect("test URI must parse"),
            ))
            .expect("encoded local file URI must resolve");
        resolution
            .file_system()
            .stat(resolution.path())
            .expect("decoded path must access the native fixture");
    }

    assert_eq!(
        Path::parse("/report final.txt").expect("test logical path must parse"),
        registry
            .resolve_config(&FileSystemConfig::new(
                ConnectionUri::parse("file:///report%20final.txt")
                    .expect("test URI must parse"),
            ))
            .expect("encoded local file URI must resolve")
            .path()
            .clone()
    );
}

/// Rooted providers reject a native authority that cannot be opened.
#[test]
fn test_rooted_local_provider_rejects_missing_root() {
    let root = std::env::temp_dir().join(format!(
        "qubit-fs-local-missing-root-{}",
        std::process::id()
    ));
    let id = FileSystemId::new("provider-missing-root")
        .expect("test identity must be valid");
    assert!(
        LocalFileSystemProvider::rooted(id, &root).is_err(),
        "a missing rooted authority must be rejected"
    );
}
