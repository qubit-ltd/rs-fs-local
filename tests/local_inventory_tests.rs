// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Link-time discovery of the standard host local provider.

use qubit_fs::directory::ListOptions;
use qubit_fs::directory::ListScope;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::path::ConnectionUri;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_registry::FileSystemConfig;
use qubit_fs_registry::FileSystemRegistry;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;

/// The linked host submission is discoverable and resolves `file:` URIs.
#[test]
fn test_inventory_discovers_host_provider() {
    let registry = FileSystemRegistry::from_inventory().expect("linked provider registry");
    assert_eq!(registry.provider_ids().len(), 1);
    assert_eq!(registry.provider_ids()[0].as_str(), "local-file");
    let config = FileSystemConfig::new(ConnectionUri::parse("file:///tmp").expect("file URI"));
    let resolution = registry.resolve_config(&config).expect("host resolution");
    assert_eq!(resolution.file_system().properties().info().provider_id(), "local-file");
}

/// The discovered host provider applies the finite standard listing depth.
#[test]
fn test_inventory_host_provider_has_standard_resource_limits() {
    let root = tempfile::tempdir().expect("isolated hierarchy");
    let mut nested = root.path().to_path_buf();
    for _ in 0..66 {
        nested.push("d");
        std::fs::create_dir(&nested).expect("nested directory");
    }
    let uri = format!("file://{}", root.path().display());
    let registry = FileSystemRegistry::from_inventory().expect("linked provider registry");
    let resolution = registry
        .resolve_config(&FileSystemConfig::new(ConnectionUri::parse(&uri).expect("host URI")))
        .expect("host resolution");
    let mut listing = resolution
        .file_system()
        .list(
            &ListScope::Path(resolution.path().clone()),
            ListOptions::default().with_recursive(true),
        )
        .expect("listing begins");
    let mut count = 0;
    while listing.next_entry().expect("bounded listing").is_some() {
        count += 1;
    }
    assert_eq!(count, 64, "standard depth ceiling stops before the deepest directories");
}

/// Rooted authorities remain caller-configured and can be registered
/// explicitly.
#[test]
fn test_inventory_does_not_submit_rooted_provider() {
    let root = tempfile::tempdir().expect("isolated root");
    let registry = FileSystemRegistry::from_inventory().expect("linked provider registry");
    let descriptor = ProviderDescriptor::new(ProviderId::new("local-rooted-test").expect("provider ID"))
        .with_aliases(["rooted-test"])
        .expect("provider alias");
    let provider = LocalFileSystemProvider::rooted_with_descriptor(
        descriptor,
        FileSystemId::new("local-rooted-test").expect("filesystem ID"),
        root.path(),
        LocalResourcePolicy::standard(),
    )
    .expect("rooted provider");
    registry.register(provider).expect("explicit rooted registration");
    assert_eq!(registry.provider_ids().len(), 2);
    assert!(
        registry
            .provider_ids()
            .iter()
            .any(|id| id.as_str() == "local-rooted-test")
    );
}
