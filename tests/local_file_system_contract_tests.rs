// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful contract coverage for the rooted local adapter.

use qubit_fs::{
    CopyOptions,
    CreateDirectoryOptions,
    FileSystem,
    FileSystemId,
    FsErrorKind,
    ListOptions,
    Path,
    WriteOptions,
};
use qubit_fs_local::LocalFileSystems;
use qubit_fs_testkit::{
    FileSystemContractSuite,
    FileSystemFixture,
    FixtureError,
    FixtureResult,
    FixtureSupport,
};

/// Isolated rooted filesystem fixture used by the provider-neutral suite.
struct RootedFixture {
    _root: tempfile::TempDir,
    file_system: FileSystem,
}

impl RootedFixture {
    /// Creates a fresh descriptor-rooted local facade.
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture root must be created");
        let id = FileSystemId::new("local-contract-root")
            .expect("fixture filesystem identity must be valid");
        let file_system = LocalFileSystems::rooted_with_id(id, root.path())
            .expect("rooted fixture filesystem must open");
        let fixture_dir =
            Path::parse("/fixture").expect("fixture path must be valid");
        file_system
            .create_directory(&fixture_dir, CreateDirectoryOptions::default())
            .expect("fixture directory must be created");
        Self {
            _root: root,
            file_system,
        }
    }
}

impl FileSystemFixture for RootedFixture {
    fn file_system(&self) -> &FileSystem {
        &self.file_system
    }

    fn path(&self, relative: &str) -> FixtureResult<Path> {
        if relative == "list-root" {
            return Path::parse("/").map_err(|error| {
                FixtureError::with_source("fixture path is invalid", error)
            });
        }
        Path::parse(&format!("/fixture/{relative}")).map_err(|error| {
            FixtureError::with_source("fixture path is invalid", error)
        })
    }

    fn seed_file(
        &self,
        relative: &str,
        bytes: &[u8],
    ) -> FixtureResult<FixtureSupport<Path>> {
        if let Some((parent, _)) = relative.rsplit_once('/') {
            let parent = Path::parse(&format!("/fixture/{parent}")).map_err(
                |error| {
                    FixtureError::with_source(
                        "fixture parent path is invalid",
                        error,
                    )
                },
            )?;
            match self.file_system.stat(&parent) {
                Ok(_) => {}
                Err(error) if error.kind() == FsErrorKind::NotFound => {
                    self.file_system
                        .create_directory(
                            &parent,
                            CreateDirectoryOptions::default(),
                        )
                        .map_err(|error| {
                            FixtureError::with_source(
                                "fixture seed parent directory failed",
                                error,
                            )
                        })?;
                }
                Err(error) => {
                    return Err(FixtureError::with_source(
                        "fixture seed parent lookup failed",
                        error,
                    ));
                }
            }
        }
        let path = self.path(relative)?;
        self.file_system
            .write_all(&path, bytes, WriteOptions::default())
            .map_err(|failure| {
                FixtureError::new(format!(
                    "fixture seed write failed: {failure}"
                ))
            })?;
        Ok(FixtureSupport::Supported(path))
    }

    fn read_file(&self, path: &Path) -> FixtureResult<FixtureSupport<Vec<u8>>> {
        self.file_system
            .read_all(path, Default::default(), 1024 * 1024)
            .map(FixtureSupport::Supported)
            .map_err(|error| {
                FixtureError::with_source("fixture read failed", error)
            })
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
        let file_system =
            LocalFileSystems::host().expect("host filesystem must open");
        Self { root, file_system }
    }

    /// Converts a fixture-relative path into the host facade's logical path.
    fn logical_path(&self, relative: &str) -> FixtureResult<Path> {
        let native = self.root.path().join(relative);
        Path::parse(
            native
                .to_str()
                .expect("test fixture paths must be valid UTF-8"),
        )
        .map_err(|error| {
            FixtureError::with_source("fixture path is invalid", error)
        })
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

    fn seed_file(
        &self,
        relative: &str,
        bytes: &[u8],
    ) -> FixtureResult<FixtureSupport<Path>> {
        if let Some((parent, _)) = relative.rsplit_once('/') {
            let parent = self.logical_path(parent)?;
            match self.file_system.stat(&parent) {
                Ok(_) => {}
                Err(error) if error.kind() == FsErrorKind::NotFound => {
                    self.file_system
                        .create_directory(
                            &parent,
                            CreateDirectoryOptions::default(),
                        )
                        .map_err(|error| {
                            FixtureError::with_source(
                                "fixture seed parent directory failed",
                                error,
                            )
                        })?;
                }
                Err(error) => {
                    return Err(FixtureError::with_source(
                        "fixture seed parent lookup failed",
                        error,
                    ));
                }
            }
        }
        let path = self.logical_path(relative)?;
        self.file_system
            .write_all(&path, bytes, WriteOptions::default())
            .map_err(|failure| {
                FixtureError::new(format!(
                    "fixture seed write failed: {failure}"
                ))
            })?;
        Ok(FixtureSupport::Supported(path))
    }

    fn read_file(&self, path: &Path) -> FixtureResult<FixtureSupport<Vec<u8>>> {
        self.file_system
            .read_all(path, Default::default(), 1024 * 1024)
            .map(FixtureSupport::Supported)
            .map_err(|error| {
                FixtureError::with_source("fixture read failed", error)
            })
    }
}

/// The rooted adapter satisfies the synchronous provider-neutral contract.
#[test]
fn test_rooted_local_adapter_passes_provider_contract() {
    let fixture = RootedFixture::new();
    FileSystemContractSuite::new(&fixture).assert_all();
}

/// The host adapter satisfies the synchronous provider-neutral contract.
#[cfg(unix)]
#[test]
fn test_host_local_adapter_passes_provider_contract() {
    let fixture = HostFixture::new();
    FileSystemContractSuite::new(&fixture).assert_all();
}

/// Rooted listings preserve the namespace of a non-root request path.
#[test]
fn test_rooted_list_keeps_entry_paths_below_requested_root() {
    let fixture = RootedFixture::new();
    let requested_root = Path::parse("/fixture/listed")
        .expect("requested listing root must be valid");
    let child = Path::parse("/fixture/listed/child")
        .expect("listed child path must be valid");
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

    let copied = Path::parse("/fixture/copy-target/child.txt")
        .expect("copied path must be valid");
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
            WriteOptions {
                content_type: Some("text/plain".to_owned()),
                ..WriteOptions::default()
            },
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
            ListOptions {
                prefix: Some("report%25name.txt".to_owned()),
                ..ListOptions::default()
            },
        )
        .expect("local adapter must accept canonical prefixes");
    let entry = stream
        .next_entry()
        .expect("listing must not fail")
        .expect("canonical prefix must match");
    assert_eq!(matching, entry.path);
}
