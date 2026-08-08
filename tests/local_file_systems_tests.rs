// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::FileSystemCapability;
use qubit_fs::FileSystemId;
use qubit_fs::Path;
use qubit_fs::PathSemantics;
use qubit_fs::SymlinkPolicy;
use qubit_fs::TempFileOptions;
use qubit_fs_local::LocalFileSystems;

/// The host factory returns a concrete hierarchical local filesystem.
#[test]
fn test_host_factory_returns_concrete_file_system() {
    let file_system =
        LocalFileSystems::host().expect("host filesystem should construct");
    assert_eq!(file_system.properties().info().provider_id(), "local-file");
    assert_eq!(
        file_system.properties().info().path_semantics(),
        PathSemantics::Hierarchical
    );
    assert_eq!(
        file_system.properties().symlink_policy(),
        SymlinkPolicy::FollowWithinFileSystem
    );
}

/// Native failures expose the adapter's canonical provider identity.
#[test]
fn test_host_stat_error_uses_canonical_provider_id() {
    let path = Path::parse(&format!(
        "/__qubit_fs_local_missing_{}",
        std::process::id()
    ))
    .expect("test path should be logical");
    let file_system =
        LocalFileSystems::host().expect("host filesystem should construct");

    let error = file_system
        .stat(&path)
        .expect_err("missing native path should fail to stat");

    assert_eq!(error.provider(), Some("local-file"));
}

/// Local capabilities expose the native atomic rename guarantee when available.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[test]
fn test_host_factory_advertises_atomic_rename() {
    let file_system =
        LocalFileSystems::host().expect("host filesystem should construct");

    assert!(
        file_system
            .properties()
            .capabilities()
            .contains(FileSystemCapability::AtomicRename)
    );
}

/// An explicitly supplied rooted filesystem identity remains unchanged.
#[test]
fn test_rooted_with_id_preserves_explicit_identity() {
    let root = tempfile::tempdir().expect("root should exist");
    let id = FileSystemId::new("local-test-root")
        .expect("filesystem id should be valid");
    let file_system = LocalFileSystems::rooted_with_id(id.clone(), root.path())
        .expect("rooted filesystem should construct");
    assert_eq!(file_system.properties().info().id(), &id);
}

/// Rooted metadata and existence probes treat the logical root as the opened
/// native authority rather than an invalid empty descendant.
#[test]
fn test_rooted_root_is_statable_and_exists() {
    let root = tempfile::tempdir().expect("root should exist");
    let file_system = LocalFileSystems::rooted(root.path())
        .expect("rooted filesystem should construct");

    assert!(
        file_system
            .stat(&Path::root())
            .expect("root stat should succeed")
            .is_directory_like()
    );
    assert!(
        file_system
            .exists(&Path::root())
            .expect("root existence probe should succeed")
    );
}

/// Both local authority modes advertise their exact portable capability set.
#[test]
fn test_local_capabilities_include_empty_directory_only_when_supported() {
    let root = tempfile::tempdir().expect("root should exist");
    let rooted = LocalFileSystems::rooted(root.path())
        .expect("rooted filesystem should construct");
    let host =
        LocalFileSystems::host().expect("host filesystem should construct");

    for file_system in [&rooted, &host] {
        let capabilities = file_system.properties().capabilities();
        assert!(capabilities.contains(FileSystemCapability::EmptyDirectory));
        assert!(!capabilities.contains(FileSystemCapability::Symlink));
        for capability in FileSystemCapability::ALL.iter().copied() {
            let expected = match capability {
                FileSystemCapability::DurableRename
                | FileSystemCapability::DurableFileCopy => cfg!(unix),
                _ => matches!(
                    capability,
                    FileSystemCapability::List
                        | FileSystemCapability::Read
                        | FileSystemCapability::Write
                        | FileSystemCapability::Append
                        | FileSystemCapability::CreateDirectory
                        | FileSystemCapability::EmptyDirectory
                        | FileSystemCapability::Delete
                        | FileSystemCapability::RecursiveDelete
                        | FileSystemCapability::Rename
                        | FileSystemCapability::Copy
                        | FileSystemCapability::TempFile
                        | FileSystemCapability::TempDirectory
                        | FileSystemCapability::AtomicRename
                        | FileSystemCapability::AtomicReplace
                        | FileSystemCapability::AtomicFileCopy
                        | FileSystemCapability::AtomicTempPersist
                ),
            };
            if !expected {
                assert!(!capabilities.contains(capability));
            }
        }
    }
}

/// Verifies provider properties expose the two durable publication protocols.
#[test]
fn test_local_provider_advertises_supported_durability() {
    let file_system =
        LocalFileSystems::host().expect("host filesystem should construct");
    let capabilities = file_system.properties().capabilities();

    assert_eq!(
        cfg!(unix),
        capabilities.contains(FileSystemCapability::DurableRename),
    );
    assert_eq!(
        cfg!(unix),
        capabilities.contains(FileSystemCapability::DurableFileCopy),
    );
}

/// Verifies automatic rooted identities are valid and distinct per facade.
#[test]
fn test_rooted_factory_assigns_distinct_process_local_identities() {
    let first_root = tempfile::tempdir().expect("first root should exist");
    let second_root = tempfile::tempdir().expect("second root should exist");

    let first = LocalFileSystems::rooted(first_root.path())
        .expect("first rooted filesystem should construct");
    let second = LocalFileSystems::rooted(second_root.path())
        .expect("second rooted filesystem should construct");

    assert_ne!(
        first.properties().info().id(),
        second.properties().info().id()
    );
}

/// Host temporary files honor the requested logical parent and name affixes.
#[cfg(unix)]
#[test]
fn test_host_temp_file_applies_parent_and_affixes() {
    let parent = tempfile::tempdir().expect("temporary parent should exist");
    let canonical_parent = std::fs::canonicalize(parent.path())
        .expect("temporary parent should canonicalize");
    let parent = Path::parse(
        canonical_parent
            .to_str()
            .expect("test temporary path should be UTF-8"),
    )
    .expect("test temporary path should be logical");
    let file_system =
        LocalFileSystems::host().expect("host filesystem should construct");

    let temporary = file_system
        .create_temp_file(
            TempFileOptions::default()
                .with_parent(Some(parent.clone()))
                .with_prefix("host-upload-".to_owned())
                .with_suffix(".part".to_owned()),
        )
        .expect("temporary file should be created");

    assert!(
        temporary
            .path()
            .as_str()
            .starts_with(&format!("{}/", parent.as_str()))
    );
    assert!(temporary.path().as_str().contains("/host-upload-"));
    assert!(temporary.path().as_str().ends_with(".part"));
}
