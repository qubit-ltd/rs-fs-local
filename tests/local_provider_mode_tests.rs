// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public behavior coverage for host and rooted provider authority modes.

use qubit_fs::error::FsErrorKind;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::path::ConnectionUri;
use qubit_fs::path::Path;
use qubit_fs::read::ReadOptions;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_registry::FileSystemConfig;
use qubit_fs_registry::FileSystemRegistry;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;
use qubit_spi::ProviderSelection;

/// Host and rooted providers expose the identity selected by their authority
/// mode.
#[test]
fn test_local_provider_modes_select_expected_authority() {
    let host_registry = FileSystemRegistry::default();
    host_registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("host provider must register");
    let host_resolution = host_registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///tmp").expect("host test URI must parse"),
        ))
        .expect("host provider must resolve an absolute file URI");
    assert_eq!(
        "local-host",
        host_resolution
            .file_system()
            .properties()
            .info()
            .id()
            .as_str()
    );

    let root = tempfile::tempdir().expect("provider root must be created");
    let rooted_id =
        FileSystemId::new("provider-mode-root").expect("rooted provider identity must be valid");
    let rooted_registry = FileSystemRegistry::default();
    rooted_registry
        .register(
            LocalFileSystemProvider::rooted(
                rooted_id.clone(),
                root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("rooted provider must open"),
        )
        .expect("rooted provider must register");
    let rooted_resolution = rooted_registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///inside").expect("rooted test URI must parse"),
        ))
        .expect("rooted provider must resolve an absolute file URI");
    assert_eq!(
        &rooted_id,
        rooted_resolution.file_system().properties().info().id()
    );
}

/// Distinct rooted provider descriptors can coexist in one registry.
#[test]
fn test_rooted_local_providers_can_use_distinct_registry_descriptors() {
    let first_root = tempfile::tempdir().expect("first root must be created");
    let second_root = tempfile::tempdir().expect("second root must be created");
    let first_descriptor = ProviderDescriptor::new(
        ProviderId::new("rooted-first").expect("provider ID must be valid"),
    )
    .with_aliases(["rooted-first-file"])
    .expect("provider alias must be valid");
    let second_descriptor = ProviderDescriptor::new(
        ProviderId::new("rooted-second").expect("provider ID must be valid"),
    )
    .with_aliases(["rooted-second-file"])
    .expect("provider alias must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted_with_descriptor(
                first_descriptor,
                FileSystemId::new("first-root").expect("filesystem ID must be valid"),
                first_root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("first rooted provider must open"),
        )
        .expect("first rooted provider must register");
    registry
        .register(
            LocalFileSystemProvider::rooted_with_descriptor(
                second_descriptor,
                FileSystemId::new("second-root").expect("filesystem ID must be valid"),
                second_root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("second rooted provider must open"),
        )
        .expect("second rooted provider must register");

    let first =
        FileSystemConfig::new(ConnectionUri::parse("file:///inside").expect("URI must parse"))
            .with_selection(
                ProviderSelection::named("rooted-first").expect("selection must parse"),
            );
    let second =
        FileSystemConfig::new(ConnectionUri::parse("file:///inside").expect("URI must parse"))
            .with_selection(
                ProviderSelection::named("rooted-second").expect("selection must parse"),
            );
    assert_eq!(
        "rooted-first",
        registry
            .resolve_config(&first)
            .expect("first provider must resolve")
            .file_system()
            .properties()
            .info()
            .provider_id(),
    );
    assert_eq!(
        "rooted-second",
        registry
            .resolve_config(&second)
            .expect("second provider must resolve")
            .file_system()
            .properties()
            .info()
            .provider_id(),
    );
}

/// Rooted authority remains part of the filesystem identity when its canonical
/// URI and provider-decoded path are shared with another rooted provider.
#[test]
fn test_rooted_provider_identity_is_scoped_beyond_canonical_uri() {
    let first_root = tempfile::tempdir().expect("first root must be created");
    let second_root = tempfile::tempdir().expect("second root must be created");
    let first_id =
        FileSystemId::new("identity-first-root").expect("first filesystem ID must be valid");
    let second_id =
        FileSystemId::new("identity-second-root").expect("second filesystem ID must be valid");
    let first_descriptor = ProviderDescriptor::new(
        ProviderId::new("identity-first").expect("first provider ID must be valid"),
    )
    .with_aliases(["identity-first-file"])
    .expect("first provider alias must be valid");
    let second_descriptor = ProviderDescriptor::new(
        ProviderId::new("identity-second").expect("second provider ID must be valid"),
    )
    .with_aliases(["identity-second-file"])
    .expect("second provider alias must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted_with_descriptor(
                first_descriptor,
                first_id.clone(),
                first_root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("first rooted provider must open"),
        )
        .expect("first rooted provider must register");
    registry
        .register(
            LocalFileSystemProvider::rooted_with_descriptor(
                second_descriptor,
                second_id.clone(),
                second_root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("second rooted provider must open"),
        )
        .expect("second rooted provider must register");

    let uri =
        ConnectionUri::parse("file:///identity-scope.txt").expect("identity-scope URI must parse");
    let first = registry
        .resolve_config(&FileSystemConfig::new(uri.clone()).with_selection(
            ProviderSelection::named("identity-first").expect("first selection must parse"),
        ))
        .expect("first rooted provider must resolve");
    let second = registry
        .resolve_config(&FileSystemConfig::new(uri).with_selection(
            ProviderSelection::named("identity-second").expect("second selection must parse"),
        ))
        .expect("second rooted provider must resolve");

    assert_eq!(first.canonical_uri(), second.canonical_uri());
    assert_eq!(first.path(), second.path());
    first
        .file_system()
        .write_all(first.path(), b"first-rooted", WriteOptions::default())
        .expect("first rooted content must be written");
    second
        .file_system()
        .write_all(second.path(), b"second-rooted", WriteOptions::default())
        .expect("second rooted content must be written");

    let first_metadata = first
        .file_system()
        .stat(first.path())
        .expect("first rooted content must be stat-able");
    let second_metadata = second
        .file_system()
        .stat(second.path())
        .expect("second rooted content must be stat-able");
    assert_eq!(Some(12), first_metadata.len());
    assert_eq!(Some(13), second_metadata.len());
    assert_eq!(
        b"first-rooted".as_slice(),
        first
            .file_system()
            .read_all(first.path(), ReadOptions::default(), 64)
            .expect("first rooted content must be readable")
            .as_slice(),
    );
    assert_eq!(
        b"second-rooted".as_slice(),
        second
            .file_system()
            .read_all(second.path(), ReadOptions::default(), 64)
            .expect("second rooted content must be readable")
            .as_slice(),
    );
    assert_eq!(&first_id, first.file_system().properties().info().id(),);
    assert_eq!(&second_id, second.file_system().properties().info().id(),);
    assert_ne!(
        first.file_system().properties().info().id(),
        second.file_system().properties().info().id(),
    );
    assert_eq!(
        "identity-first",
        first.file_system().properties().info().provider_id(),
    );
    assert_eq!(
        "identity-second",
        second.file_system().properties().info().provider_id(),
    );
    assert_ne!(
        first.file_system().properties().info().provider_id(),
        second.file_system().properties().info().provider_id(),
    );

    let host_registry = FileSystemRegistry::default();
    host_registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("host provider must register");
    let host_replay = host_registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse(first.canonical_uri().as_str())
                .expect("canonical URI must replay as a connection URI"),
        ))
        .expect("host provider must resolve the replayed URI");
    assert_eq!(first.canonical_uri(), host_replay.canonical_uri());
    assert_eq!(first.path(), host_replay.path());
    assert_eq!(
        "local-host",
        host_replay.file_system().properties().info().id().as_str(),
    );
    assert_eq!(
        "local-file",
        host_replay.file_system().properties().info().provider_id(),
    );
    assert_ne!(
        first.file_system().properties().info().id(),
        host_replay.file_system().properties().info().id(),
    );
    assert_ne!(
        first.file_system().properties().info().provider_id(),
        host_replay.file_system().properties().info().provider_id(),
    );
}

/// Errors from a rooted provider retain the descriptor identity selected by
/// the registry rather than falling back to the default local provider ID.
#[test]
fn test_rooted_provider_errors_retain_descriptor_identity() {
    let root = tempfile::tempdir().expect("provider root must be created");
    let descriptor = ProviderDescriptor::new(
        ProviderId::new("rooted-errors").expect("provider ID must be valid"),
    )
    .with_aliases(["rooted-errors-file"])
    .expect("provider alias must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted_with_descriptor(
                descriptor,
                FileSystemId::new("errors-root").expect("filesystem ID must be valid"),
                root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("rooted provider must open"),
        )
        .expect("rooted provider must register");
    let resolution = registry
        .resolve_config(
            &FileSystemConfig::new(
                ConnectionUri::parse("file:///missing").expect("URI must parse"),
            )
            .with_selection(
                ProviderSelection::named("rooted-errors").expect("selection must parse"),
            ),
        )
        .expect("provider must resolve");

    let error = resolution
        .file_system()
        .stat(&Path::parse("/missing").expect("path must parse"))
        .expect_err("missing path must fail");
    assert_eq!(error.kind(), FsErrorKind::NotFound);
    assert_eq!(error.provider(), Some("rooted-errors"));
}
