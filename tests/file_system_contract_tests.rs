// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::{
    FileSystem,
    FileSystemId,
    FsPath,
};
use qubit_fs_local::{
    LocalFileSystem,
    RootedLocalFileSystem,
};
use qubit_fs_testkit::FileSystemFixture;
use tempfile::TempDir;

struct HostFixture {
    _directory: TempDir,
    file_system: LocalFileSystem,
}

impl HostFixture {
    /// Creates an isolated host-filesystem fixture.
    fn new() -> Self {
        Self {
            _directory: tempfile::tempdir()
                .expect("the host contract root should be created"),
            file_system: LocalFileSystem::host(),
        }
    }
}

impl FileSystemFixture for HostFixture {
    fn file_system(&self) -> &dyn FileSystem {
        &self.file_system
    }

    fn path(&self, relative: &str) -> FsPath {
        LocalFileSystem::path_from_native(
            &self._directory.path().join(relative),
        )
        .expect("host contract paths should convert")
    }
}

struct RootedFixture {
    _directory: TempDir,
    file_system: RootedLocalFileSystem,
}

impl RootedFixture {
    /// Creates an isolated rooted-filesystem fixture.
    fn new() -> Self {
        let directory = tempfile::tempdir()
            .expect("the rooted contract root should be created");
        let id = FileSystemId::new("contract-rooted")
            .expect("the rooted contract ID should validate");
        let file_system = RootedLocalFileSystem::open(id, directory.path())
            .expect("the rooted contract filesystem should open");
        Self {
            _directory: directory,
            file_system,
        }
    }
}

impl FileSystemFixture for RootedFixture {
    fn file_system(&self) -> &dyn FileSystem {
        &self.file_system
    }

    fn path(&self, relative: &str) -> FsPath {
        FsPath::parse(&format!("/{relative}"))
            .expect("rooted contract paths should parse")
    }
}

/// Verifies host filesystem properties satisfy the shared contract.
#[test]
fn test_host_properties_contract() {
    qubit_fs_testkit::assert_properties_contract(&HostFixture::new());
}

/// Verifies host filesystem capabilities satisfy the shared contract.
#[test]
fn test_host_capabilities_contract() {
    qubit_fs_testkit::assert_capabilities_contract(&HostFixture::new());
}

/// Verifies host metadata behavior satisfies the shared contract.
#[test]
fn test_host_stat_contract() {
    qubit_fs_testkit::assert_stat_contract(&HostFixture::new());
}

/// Verifies host read behavior satisfies the shared contract.
#[test]
fn test_host_read_contract() {
    qubit_fs_testkit::assert_read_contract(&HostFixture::new());
}

/// Verifies host write behavior satisfies the shared contract.
#[test]
fn test_host_write_contract() {
    qubit_fs_testkit::assert_write_contract(&HostFixture::new());
}

/// Verifies host append behavior satisfies the shared contract.
#[test]
fn test_host_append_contract() {
    qubit_fs_testkit::assert_append_contract(&HostFixture::new());
}

/// Verifies host atomic replacement satisfies the shared contract.
#[test]
fn test_host_atomic_replace_contract() {
    qubit_fs_testkit::assert_atomic_replace_contract(&HostFixture::new());
}

/// Verifies host option preflight satisfies the shared contract.
#[test]
fn test_host_preflight_contract() {
    qubit_fs_testkit::assert_preflight_contract(&HostFixture::new());
}

/// Verifies host unsupported operations satisfy the shared contract.
#[test]
fn test_host_unsupported_operations_contract() {
    qubit_fs_testkit::assert_unsupported_operations_contract(
        &HostFixture::new(),
    );
}

/// Verifies rooted filesystem properties satisfy the shared contract.
#[test]
fn test_rooted_properties_contract() {
    qubit_fs_testkit::assert_properties_contract(&RootedFixture::new());
}

/// Verifies rooted filesystem capabilities satisfy the shared contract.
#[test]
fn test_rooted_capabilities_contract() {
    qubit_fs_testkit::assert_capabilities_contract(&RootedFixture::new());
}

/// Verifies rooted metadata behavior satisfies the shared contract.
#[test]
fn test_rooted_stat_contract() {
    qubit_fs_testkit::assert_stat_contract(&RootedFixture::new());
}

/// Verifies rooted read behavior satisfies the shared contract.
#[test]
fn test_rooted_read_contract() {
    qubit_fs_testkit::assert_read_contract(&RootedFixture::new());
}

/// Verifies rooted write behavior satisfies the shared contract.
#[test]
fn test_rooted_write_contract() {
    qubit_fs_testkit::assert_write_contract(&RootedFixture::new());
}

/// Verifies rooted append behavior satisfies the shared contract.
#[test]
fn test_rooted_append_contract() {
    qubit_fs_testkit::assert_append_contract(&RootedFixture::new());
}

/// Verifies rooted atomic replacement satisfies the shared contract.
#[test]
fn test_rooted_atomic_replace_contract() {
    qubit_fs_testkit::assert_atomic_replace_contract(&RootedFixture::new());
}

/// Verifies rooted option preflight satisfies the shared contract.
#[test]
fn test_rooted_preflight_contract() {
    qubit_fs_testkit::assert_preflight_contract(&RootedFixture::new());
}

/// Verifies rooted unsupported operations satisfy the shared contract.
#[test]
fn test_rooted_unsupported_operations_contract() {
    qubit_fs_testkit::assert_unsupported_operations_contract(
        &RootedFixture::new(),
    );
}
