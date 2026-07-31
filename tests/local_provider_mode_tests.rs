// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public behavior coverage for host and rooted provider authority modes.

use qubit_fs::{
    ConnectionUri,
    FileSystemId,
};
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_registry::{
    FileSystemConfig,
    FileSystemRegistry,
};

/// Host and rooted providers expose the identity selected by their authority
/// mode.
#[test]
fn test_local_provider_modes_select_expected_authority() {
    let host_registry = FileSystemRegistry::default();
    host_registry
        .register(LocalFileSystemProvider::new())
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
        .register(LocalFileSystemProvider::rooted(
            rooted_id.clone(),
            root.path(),
        ).expect("rooted provider must open"))
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
