// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful contract coverage for the rooted local adapter.

use std::path::PathBuf;

use qubit_fs::CopyOptions;
use qubit_fs::CreateDirectoryOptions;
use qubit_fs::FileSystem;
use qubit_fs::FileSystemId;
use qubit_fs::FsErrorKind;
use qubit_fs::ListOptions;
use qubit_fs::Path;
use qubit_fs::WriteOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::host_path_to_logical;
use qubit_fs_testkit::FileSystemFixture;
use qubit_fs_testkit::FixtureError;
use qubit_fs_testkit::FixtureResult;
use qubit_fs_testkit::FixtureSupport;
use qubit_fs_testkit::register_file_system_contract_tests;

/// Isolated rooted filesystem fixture used by the provider-neutral suite.
struct RootedFixture {
    root: tempfile::TempDir,
    file_system: FileSystem,
}

impl RootedFixture {
    /// Creates a fresh descriptor-rooted local facade.
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture root must be created");
        std::fs::create_dir(root.path().join("fixture"))
            .expect("fixture directory must be created");
        let id = FileSystemId::new("local-contract-root")
            .expect("fixture filesystem identity must be valid");
        let file_system = LocalFileSystems::rooted_with_id(id, root.path())
            .expect("rooted fixture filesystem must open");
        Self { root, file_system }
    }

    /// Converts a rooted logical path into its independent native observation
    /// path.
    fn native_path(&self, path: &Path) -> FixtureResult<PathBuf> {
        let relative = path
            .as_str()
            .strip_prefix('/')
            .ok_or_else(|| FixtureError::new("rooted fixture path must be absolute"))?;
        Ok(self.root.path().join(relative))
    }
}

impl FileSystemFixture for RootedFixture {
    fn file_system(&self) -> &FileSystem {
        &self.file_system
    }

    fn path(&self, relative: &str) -> FixtureResult<Path> {
        if relative == "list-root" {
            return Path::parse("/")
                .map_err(|error| FixtureError::with_source("fixture path is invalid", error));
        }
        Path::parse(&format!("/fixture/{relative}"))
            .map_err(|error| FixtureError::with_source("fixture path is invalid", error))
    }

    fn seed_file(&self, relative: &str, bytes: &[u8]) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.path(relative)?;
        let native = self.native_path(&path)?;
        if let Some(parent) = native.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                FixtureError::with_source("fixture seed parent directory failed", error)
            })?;
        }
        std::fs::write(native, bytes)
            .map_err(|error| FixtureError::with_source("fixture seed write failed", error))?;
        Ok(FixtureSupport::Supported(path))
    }

    fn seed_empty_directory(&self, relative: &str) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.path(relative)?;
        std::fs::create_dir_all(self.native_path(&path)?).map_err(|error| {
            FixtureError::with_source("fixture directory creation failed", error)
        })?;
        Ok(FixtureSupport::Supported(path))
    }

    fn read_file(&self, path: &Path) -> FixtureResult<FixtureSupport<Vec<u8>>> {
        std::fs::read(self.native_path(path)?)
            .map(FixtureSupport::Supported)
            .map_err(|error| FixtureError::with_source("fixture read failed", error))
    }
}

/// Isolated host filesystem fixture used to exercise the host SPI path.
#[cfg(unix)]
struct HostFixture {
    root: tempfile::TempDir,
    file_system: FileSystem,
}

#[cfg(unix)]
impl HostFixture {
    /// Creates a fresh host facade rooted at an isolated native directory.
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture root must be created");
        let file_system = LocalFileSystems::host().expect("host filesystem must open");
        Self { root, file_system }
    }

    /// Converts a fixture-relative path into the host facade's logical path.
    fn logical_path(&self, relative: &str) -> FixtureResult<Path> {
        let native = self.root.path().join(relative);
        host_path_to_logical(&native)
            .map_err(|error| FixtureError::with_source("fixture path is invalid", error))
    }

    /// Converts a host logical path into its independent native observation
    /// path.
    fn native_path(&self, path: &Path) -> PathBuf {
        PathBuf::from(path.as_str())
    }
}

#[cfg(unix)]
impl FileSystemFixture for HostFixture {
    fn file_system(&self) -> &FileSystem {
        &self.file_system
    }

    fn path(&self, relative: &str) -> FixtureResult<Path> {
        self.logical_path(relative)
    }

    fn seed_file(&self, relative: &str, bytes: &[u8]) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.logical_path(relative)?;
        let native = self.native_path(&path);
        if let Some(parent) = native.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                FixtureError::with_source("fixture seed parent directory failed", error)
            })?;
        }
        std::fs::write(native, bytes)
            .map_err(|error| FixtureError::with_source("fixture seed write failed", error))?;
        Ok(FixtureSupport::Supported(path))
    }

    fn seed_empty_directory(&self, relative: &str) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.logical_path(relative)?;
        std::fs::create_dir_all(self.native_path(&path)).map_err(|error| {
            FixtureError::with_source("fixture directory creation failed", error)
        })?;
        Ok(FixtureSupport::Supported(path))
    }

    fn read_file(&self, path: &Path) -> FixtureResult<FixtureSupport<Vec<u8>>> {
        std::fs::read(self.native_path(path))
            .map(FixtureSupport::Supported)
            .map_err(|error| FixtureError::with_source("fixture read failed", error))
    }
}

#[cfg(unix)]
register_file_system_contract_tests! {
    module: host_contracts,
    fixture: super::HostFixture::new,
}

register_file_system_contract_tests! {
    module: rooted_contracts,
    fixture: super::RootedFixture::new,
}

/// Rooted listings preserve the namespace of a non-root request path.
#[test]
fn test_rooted_list_keeps_entry_paths_below_requested_root() {
    let fixture = RootedFixture::new();
    let requested_root =
        Path::parse("/fixture/listed").expect("requested listing root must be valid");
    let child = Path::parse("/fixture/listed/child").expect("listed child path must be valid");
    fixture
        .file_system()
        .create_directory(&requested_root, CreateDirectoryOptions::default())
        .expect("requested listing root must be created");
    fixture
        .file_system()
        .write_all(&child, b"listed", WriteOptions::default())
        .expect("listed child must be written");

    let mut stream = fixture
        .file_system()
        .list(&requested_root, ListOptions::default())
        .expect("requested directory must be listed");
    let entry = stream
        .next_entry()
        .expect("listing must not fail")
        .expect("listing must return the child");

    assert_eq!(child, entry.path);
    assert!(
        stream
            .next_entry()
            .expect("listing must complete without failure")
            .is_none()
    );
}

/// Root listings report child paths beneath the rooted logical namespace.
#[test]
fn test_rooted_list_keeps_entry_paths_below_root_request() {
    let fixture = RootedFixture::new();
    let requested_root = Path::root();

    let mut stream = fixture
        .file_system()
        .list(&requested_root, ListOptions::default())
        .expect("root directory must be listed");
    let entry = stream
        .next_entry()
        .expect("listing must not fail")
        .expect("listing must return the fixture directory");

    assert_eq!(
        Path::parse("/fixture").expect("fixture path must be valid"),
        entry.path
    );
    assert!(
        stream
            .next_entry()
            .expect("listing must complete without failure")
            .is_none()
    );
}

/// Auto copy treats a local directory source as a tree instead of rejecting it.
#[test]
fn test_rooted_copy_auto_detects_directory_sources() {
    let fixture = RootedFixture::new();
    let source = fixture
        .path("copy-source")
        .expect("source path must be valid");
    let child = fixture
        .path("copy-source/child.txt")
        .expect("child path must be valid");
    let target = fixture
        .path("copy-target")
        .expect("target path must be valid");
    fixture
        .file_system()
        .create_directory(&source, CreateDirectoryOptions::default())
        .expect("source directory must be created");
    fixture
        .file_system()
        .write_all(&child, b"contents", WriteOptions::default())
        .expect("source child must be written");

    fixture
        .file_system()
        .copy(&source, &target, CopyOptions::default())
        .expect("automatic copy must handle directory sources");

    let copied = Path::parse("/fixture/copy-target/child.txt").expect("copied path must be valid");
    assert_eq!(
        fixture
            .file_system()
            .read_all(&copied, Default::default(), 1024)
            .expect("copied child must be readable"),
        b"contents"
    );
}

/// Local-only options are rejected before the adapter opens a writer.
#[test]
fn test_rooted_write_rejects_unrepresentable_metadata_options() {
    let fixture = RootedFixture::new();
    let path = fixture.path("typed.txt").expect("path must be valid");
    let error = fixture
        .file_system()
        .write_all(
            &path,
            b"contents",
            WriteOptions::default().with_content_type(Some("text/plain".to_owned())),
        )
        .expect_err("local adapter must reject metadata it cannot retain");

    assert_eq!(error.error().kind(), FsErrorKind::RequirementNotMet);
    assert!(
        !fixture
            .file_system()
            .exists(&path)
            .expect("rejected writer must not create a file")
    );
}

/// Verifies list prefixes use canonical logical escaping rather than lossy
/// native-path text.
#[test]
fn test_rooted_list_matches_canonical_escaped_prefix() {
    let fixture = RootedFixture::new();
    let root = fixture.path("escaped-prefix").expect("path must be valid");
    let matching = fixture
        .path("escaped-prefix/report%25name.txt")
        .expect("path must be valid");
    fixture
        .file_system()
        .create_directory(&root, CreateDirectoryOptions::default())
        .expect("listing root must be created");
    fixture
        .file_system()
        .write_all(&matching, b"escaped", WriteOptions::default())
        .expect("matching file must be written");

    let mut stream = fixture
        .file_system()
        .list(
            &root,
            ListOptions::default().with_prefix(Some("report%25name.txt".to_owned())),
        )
        .expect("local adapter must accept canonical prefixes");
    let entry = stream
        .next_entry()
        .expect("listing must not fail")
        .expect("canonical prefix must match");
    assert_eq!(matching, entry.path);
}
