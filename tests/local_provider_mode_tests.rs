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
            ConnectionUri::parse("file:///tmp")
                .expect("host test URI must parse"),
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
    let rooted_id = FileSystemId::new("provider-mode-root")
        .expect("rooted provider identity must be valid");
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
            ConnectionUri::parse("file:///inside")
                .expect("rooted test URI must parse"),
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
                FileSystemId::new("first-root")
                    .expect("filesystem ID must be valid"),
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
                FileSystemId::new("second-root")
                    .expect("filesystem ID must be valid"),
                second_root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("second rooted provider must open"),
        )
        .expect("second rooted provider must register");

    let first = FileSystemConfig::new(
        ConnectionUri::parse("file:///inside").expect("URI must parse"),
    )
    .with_selection(
        ProviderSelection::named("rooted-first").expect("selection must parse"),
    );
    let second = FileSystemConfig::new(
        ConnectionUri::parse("file:///inside").expect("URI must parse"),
    )
    .with_selection(
        ProviderSelection::named("rooted-second")
            .expect("selection must parse"),
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
                FileSystemId::new("errors-root")
                    .expect("filesystem ID must be valid"),
                root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("rooted provider must open"),
        )
        .expect("rooted provider must register");
    let resolution = registry
        .resolve_config(
            &FileSystemConfig::new(
                ConnectionUri::parse("file:///missing")
                    .expect("URI must parse"),
            )
            .with_selection(
                ProviderSelection::named("rooted-errors")
                    .expect("selection must parse"),
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
