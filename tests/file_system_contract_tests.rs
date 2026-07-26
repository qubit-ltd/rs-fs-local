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

qubit_fs_testkit::sync_file_system_contract_tests!(
    host,
    super::HostFixture::new(),
);
qubit_fs_testkit::sync_file_system_contract_tests!(
    rooted,
    super::RootedFixture::new(),
);
