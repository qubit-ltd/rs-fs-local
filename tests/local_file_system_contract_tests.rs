// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful contract coverage for the rooted local adapter.

use std::fs;
use std::io::ErrorKind;
#[cfg(unix)]
use std::path::Component;
use std::path::Path as NativePath;
use std::path::PathBuf;

use qubit_fs::FileSystem;
use qubit_fs::copy::CopyOptions;
use qubit_fs::directory::CreateDirectoryOptions;
use qubit_fs::directory::ListOptions;
use qubit_fs::error::FsErrorKind;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::path::Path;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;
use qubit_fs_testkit::FileSystemContract;
use qubit_fs_testkit::FileSystemContractSuite;
use qubit_fs_testkit::FileSystemFixture;
use qubit_fs_testkit::FixtureError;
use qubit_fs_testkit::FixtureResult;
use qubit_fs_testkit::FixtureSupport;
use qubit_fs_testkit::register_file_system_contract_tests;
use qubit_local_files::path::LocalPaths;

/// Removes one native entry without following symbolic links.
fn remove_entry(path: &NativePath) -> FixtureResult<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(FixtureError::with_source("fixture teardown metadata failed", error));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        fs::remove_file(path)
            .map_err(|error| FixtureError::with_source("fixture teardown entry removal failed", error))?;
        return Ok(());
    }
    for entry in fs::read_dir(path)
        .map_err(|error| FixtureError::with_source("fixture teardown directory read failed", error))?
    {
        let entry = entry.map_err(|error| FixtureError::with_source("fixture teardown entry read failed", error))?;
        remove_entry(&entry.path())?;
    }
    fs::remove_dir(path).map_err(|error| FixtureError::with_source("fixture teardown directory removal failed", error))
}

/// Verifies that a teardown base is an actual directory rather than a link.
fn ensure_real_directory(path: &NativePath, description: &str) -> FixtureResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(FixtureError::new(format!("{description} is a symbolic link")))
        }
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(FixtureError::new(format!("{description} is not a directory"))),
        Err(error) => Err(FixtureError::with_source(
            format!("{description} metadata failed"),
            error,
        )),
    }
}

/// Removes all children of one native directory while preserving the directory.
fn clear_children(path: &NativePath) -> FixtureResult<()> {
    ensure_real_directory(path, "fixture teardown base")?;
    for entry in fs::read_dir(path)
        .map_err(|error| FixtureError::with_source("fixture teardown directory read failed", error))?
    {
        let entry = entry.map_err(|error| FixtureError::with_source("fixture teardown entry read failed", error))?;
        remove_entry(&entry.path())?;
    }
    Ok(())
}

/// Ensures a converted path remains in the fixture root and has no symlinked
/// parent that could redirect an out-of-band operation.
fn ensure_owned_path(root: &NativePath, path: &NativePath) -> FixtureResult<()> {
    ensure_real_directory(root, "fixture root")?;
    if !path.starts_with(root) {
        return Err(FixtureError::new("fixture path conversion escaped the temporary root"));
    }
    let mut parent = path.parent();
    while let Some(current) = parent {
        if current == root {
            break;
        }
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FixtureError::new("fixture path contains a symlinked parent"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(FixtureError::with_source("fixture path parent metadata failed", error));
            }
        }
        parent = current.parent();
    }
    Ok(())
}

/// Rejects a final symbolic link before an out-of-band file operation.
fn reject_symlink_target(path: &NativePath, description: &str) -> FixtureResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(FixtureError::new(format!("{description} is a symbolic link")))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(FixtureError::with_source("fixture target metadata failed", error)),
    }
}

/// Isolated rooted filesystem fixture used by the provider-neutral suite.
struct RootedFixture {
    root: tempfile::TempDir,
    file_system: FileSystem,
}

impl RootedFixture {
    /// Creates a fresh descriptor-rooted local facade.
    fn new() -> Self {
        let root = tempfile::tempdir().expect("fixture root must be created");
        std::fs::create_dir(root.path().join("fixture")).expect("fixture directory must be created");
        let id = FileSystemId::new("local-contract-root").expect("fixture filesystem identity must be valid");
        let file_system = LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted fixture filesystem must open");
        Self { root, file_system }
    }

    /// Converts a rooted logical path into its independent native observation
    /// path.
    fn native_path(&self, path: &Path) -> FixtureResult<PathBuf> {
        let virtual_path = LocalPaths::rooted()
            .from_canonical_components(path.components())
            .map_err(|error| FixtureError::with_source("rooted fixture path conversion failed", error))?;
        let relative = virtual_path
            .strip_prefix(NativePath::new("/"))
            .map_err(|error| FixtureError::with_source("rooted fixture path is not namespace relative", error))?;
        let native = self.root.path().join(relative);
        ensure_owned_path(self.root.path(), &native)?;
        Ok(native)
    }

    /// Removes fixture-created descendants while retaining the rooted facade
    /// authority and its fixed `/fixture` namespace directory.
    fn teardown_entries(&self) -> FixtureResult<()> {
        ensure_real_directory(self.root.path(), "rooted fixture root")?;
        let fixture = self.root.path().join("fixture");
        ensure_real_directory(&fixture, "rooted fixture namespace")?;
        clear_children(&fixture)?;
        for entry in fs::read_dir(self.root.path())
            .map_err(|error| FixtureError::with_source("rooted fixture root read failed", error))?
        {
            let entry = entry.map_err(|error| FixtureError::with_source("rooted fixture entry read failed", error))?;
            if entry.file_name() != "fixture" {
                remove_entry(&entry.path())?;
            }
        }
        Ok(())
    }
}

impl FileSystemFixture for RootedFixture {
    fn file_system(&self) -> &FileSystem {
        &self.file_system
    }

    fn path(&self, relative: &str) -> FixtureResult<Path> {
        Path::parse(&format!("/fixture/{relative}"))
            .map_err(|error| FixtureError::with_source("fixture path is invalid", error))
    }

    fn seed_file(&self, relative: &str, bytes: &[u8]) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.path(relative)?;
        let native = self.native_path(&path)?;
        reject_symlink_target(&native, "fixture seed target")?;
        if let Some(parent) = native.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| FixtureError::with_source("fixture seed parent directory failed", error))?;
        }
        std::fs::write(native, bytes).map_err(|error| FixtureError::with_source("fixture seed write failed", error))?;
        Ok(FixtureSupport::Supported(path))
    }

    fn seed_empty_directory(&self, relative: &str) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.path(relative)?;
        let native = self.native_path(&path)?;
        reject_symlink_target(&native, "fixture directory target")?;
        fs::create_dir_all(native)
            .map_err(|error| FixtureError::with_source("fixture directory creation failed", error))?;
        Ok(FixtureSupport::Supported(path))
    }

    fn read_file(&self, path: &Path) -> FixtureResult<FixtureSupport<Vec<u8>>> {
        let native = self.native_path(path)?;
        reject_symlink_target(&native, "fixture observation target")?;
        fs::read(native)
            .map(FixtureSupport::Supported)
            .map_err(|error| FixtureError::with_source("fixture read failed", error))
    }

    fn exists_out_of_band(&self, path: &Path) -> FixtureResult<FixtureSupport<bool>> {
        let native = self.native_path(path)?;
        match fs::symlink_metadata(native) {
            Ok(_) => Ok(FixtureSupport::Supported(true)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(FixtureSupport::Supported(false)),
            Err(error) => Err(FixtureError::with_source("fixture existence observation failed", error)),
        }
    }

    fn write_file_out_of_band(&self, path: &Path, bytes: &[u8]) -> FixtureResult<FixtureSupport<()>> {
        let native = self.native_path(path)?;
        reject_symlink_target(&native, "fixture out-of-band write target")?;
        if let Some(parent) = native.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| FixtureError::with_source("fixture out-of-band parent creation failed", error))?;
        }
        fs::write(native, bytes)
            .map_err(|error| FixtureError::with_source("fixture out-of-band write failed", error))?;
        Ok(FixtureSupport::Supported(()))
    }

    fn teardown(&self) -> FixtureResult<()> {
        self.teardown_entries()?;
        Ok(())
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
        let file_system = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host filesystem must open");
        Self { root, file_system }
    }

    /// Converts a fixture-relative path into the host facade's logical path.
    fn logical_path(&self, relative: &str) -> FixtureResult<Path> {
        let relative_path = NativePath::new(relative);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(FixtureError::new(
                "host fixture path must stay relative to its temporary root",
            ));
        }
        ensure_real_directory(self.root.path(), "host fixture root")?;
        let native = self.root.path().join(relative);
        ensure_owned_path(self.root.path(), &native)?;
        host_path_to_logical(&native).map_err(|error| FixtureError::with_source("fixture path is invalid", error))
    }

    /// Converts a host logical path into its independent native observation
    /// path.
    fn native_path(&self, path: &Path) -> FixtureResult<PathBuf> {
        let native = LocalPaths::host()
            .from_canonical_components(path.components())
            .map_err(|error| FixtureError::with_source("host fixture path conversion failed", error))?;
        ensure_owned_path(self.root.path(), &native)?;
        Ok(native)
    }

    /// Converts any host logical path returned by the provider into a native
    /// path for read-only contract observation.
    ///
    /// Unlike fixture mutation paths, a provider-generated default temporary
    /// path may legitimately live in the system temporary directory rather
    /// than below this fixture's private root.
    fn observation_path(&self, path: &Path) -> FixtureResult<PathBuf> {
        LocalPaths::host()
            .from_canonical_components(path.components())
            .map_err(|error| FixtureError::with_source("host observation path conversion failed", error))
    }

    /// Removes every fixture-created child while retaining the TempDir root.
    fn teardown_entries(&self) -> FixtureResult<()> {
        clear_children(self.root.path())
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
        let native = self.native_path(&path)?;
        reject_symlink_target(&native, "fixture directory target")?;
        reject_symlink_target(&native, "fixture seed target")?;
        if let Some(parent) = native.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| FixtureError::with_source("fixture seed parent directory failed", error))?;
        }
        std::fs::write(native, bytes).map_err(|error| FixtureError::with_source("fixture seed write failed", error))?;
        Ok(FixtureSupport::Supported(path))
    }

    fn seed_empty_directory(&self, relative: &str) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.logical_path(relative)?;
        let native = self.native_path(&path)?;
        reject_symlink_target(&native, "fixture directory target")?;
        fs::create_dir_all(native)
            .map_err(|error| FixtureError::with_source("fixture directory creation failed", error))?;
        Ok(FixtureSupport::Supported(path))
    }

    fn read_file(&self, path: &Path) -> FixtureResult<FixtureSupport<Vec<u8>>> {
        let native = self.native_path(path)?;
        reject_symlink_target(&native, "fixture observation target")?;
        fs::read(native)
            .map(FixtureSupport::Supported)
            .map_err(|error| FixtureError::with_source("fixture read failed", error))
    }

    fn exists_out_of_band(&self, path: &Path) -> FixtureResult<FixtureSupport<bool>> {
        let native = self.observation_path(path)?;
        match fs::symlink_metadata(native) {
            Ok(_) => Ok(FixtureSupport::Supported(true)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(FixtureSupport::Supported(false)),
            Err(error) => Err(FixtureError::with_source("fixture existence observation failed", error)),
        }
    }

    fn write_file_out_of_band(&self, path: &Path, bytes: &[u8]) -> FixtureResult<FixtureSupport<()>> {
        let native = self.native_path(path)?;
        reject_symlink_target(&native, "fixture out-of-band write target")?;
        if let Some(parent) = native.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| FixtureError::with_source("fixture out-of-band parent creation failed", error))?;
        }
        fs::write(native, bytes)
            .map_err(|error| FixtureError::with_source("fixture out-of-band write failed", error))?;
        Ok(FixtureSupport::Supported(()))
    }

    fn teardown(&self) -> FixtureResult<()> {
        self.teardown_entries()?;
        Ok(())
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
    let requested_root = Path::parse("/fixture/listed").expect("requested listing root must be valid");
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

    assert_eq!(Path::parse("/fixture").expect("fixture path must be valid"), entry.path);
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
    let source = fixture.path("copy-source").expect("source path must be valid");
    let child = fixture.path("copy-source/child.txt").expect("child path must be valid");
    let target = fixture.path("copy-target").expect("target path must be valid");
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

/// List-phase teardown removes generated descendants while retaining the
/// rooted fixture namespace and its facade authority.
#[test]
fn test_rooted_list_contract_teardown_preserves_fixture_root() {
    let fixture = RootedFixture::new();
    let mut suite = FileSystemContractSuite::new(&fixture);
    suite.run_contract(FileSystemContract::List).assert_satisfied();

    let fixture_root = fixture.root.path().join("fixture");
    assert!(fixture_root.is_dir(), "fixture namespace root was removed");
    assert_eq!(
        fs::read_dir(fixture_root)
            .expect("fixture namespace must remain readable")
            .count(),
        0,
        "list phase left generated descendants behind",
    );
    assert_eq!(
        fs::read_dir(fixture.root.path())
            .expect("rooted fixture root must remain readable")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsString::from("fixture")],
        "rooted list phase left root descendants behind",
    );
}

/// Host-phase teardown removes generated descendants from its isolated root.
#[cfg(unix)]
#[test]
fn test_host_list_contract_teardown_clears_fixture_root() {
    let fixture = HostFixture::new();
    let mut suite = FileSystemContractSuite::new(&fixture);
    suite.run_contract(FileSystemContract::List).assert_satisfied();

    assert_eq!(
        fs::read_dir(fixture.root.path())
            .expect("host fixture root must remain readable")
            .count(),
        0,
        "list phase left generated descendants behind",
    );
}

/// Host fixture paths reject native absolute and parent traversal inputs.
#[cfg(unix)]
#[test]
fn test_host_fixture_rejects_paths_outside_temporary_root() {
    let fixture = HostFixture::new();

    assert!(fixture.logical_path("/tmp/outside").is_err());
    assert!(fixture.logical_path("../outside").is_err());

    let outside = Path::parse("/tmp/outside").expect("outside path must be valid");
    assert!(fixture.native_path(&outside).is_err());
}

/// Teardown refuses to follow a rooted namespace symlink.
#[cfg(unix)]
#[test]
fn test_rooted_teardown_rejects_symlinked_namespace() {
    use std::os::unix::fs::symlink;

    let fixture = RootedFixture::new();
    let outside = tempfile::tempdir().expect("outside directory must be created");
    let marker = outside.path().join("must-remain");
    fs::write(&marker, b"marker").expect("outside marker must be created");

    let namespace = fixture.root.path().join("fixture");
    fs::remove_dir(&namespace).expect("fixture namespace must be removable");
    symlink(outside.path(), &namespace).expect("namespace symlink must be created");

    assert!(fixture.teardown().is_err());
    assert!(marker.exists(), "teardown followed the namespace symlink");
}

/// Teardown refuses to follow a host root symlink.
#[cfg(unix)]
#[test]
fn test_host_teardown_rejects_symlinked_root() {
    use std::os::unix::fs::symlink;

    let fixture = HostFixture::new();
    let original_root = fixture.root.path().to_path_buf();
    let moved_root = original_root.with_extension("moved");
    let outside = tempfile::tempdir().expect("outside directory must be created");
    let marker = outside.path().join("must-remain");
    fs::write(&marker, b"marker").expect("outside marker must be created");

    fs::rename(&original_root, &moved_root).expect("fixture root must be moved");
    symlink(outside.path(), &original_root).expect("root symlink must be created");

    assert!(fixture.teardown().is_err());
    assert!(marker.exists(), "teardown followed the root symlink");

    fs::remove_file(&original_root).expect("root symlink must be removed");
    fs::rename(&moved_root, &original_root).expect("fixture root must be restored");
}

/// Teardown also removes staging paths that were never registered by a suite.
#[test]
fn test_rooted_teardown_reclaims_partial_setup_and_is_idempotent() {
    let fixture = RootedFixture::new();
    let staging = fixture.root.path().join("unregistered-staging");
    fs::create_dir_all(staging.join("nested")).expect("partial staging directory");
    fs::write(staging.join("nested/data"), b"partial").expect("partial staging bytes");
    assert!(matches!(
        fixture.seed_file("partial-setup", b"seeded").expect("independent seed"),
        FixtureSupport::Supported(_)
    ));
    fixture.teardown().expect("first teardown");
    fixture.teardown().expect("idempotent teardown");
    assert!(!staging.exists());
    assert!(fixture.root.path().join("fixture").is_dir());
    assert_eq!(
        fs::read_dir(fixture.root.path().join("fixture"))
            .expect("namespace remains")
            .count(),
        0
    );
}

/// Host teardown retains only the isolated root after a partially prepared run.
#[cfg(unix)]
#[test]
fn test_host_teardown_reclaims_partial_setup_and_is_idempotent() {
    let fixture = HostFixture::new();
    let staging = fixture.root.path().join("unregistered-staging");
    fs::create_dir_all(staging.join("nested")).expect("partial staging directory");
    fs::write(staging.join("nested/data"), b"partial").expect("partial staging bytes");
    assert!(matches!(
        fixture.seed_file("partial-setup", b"seeded").expect("independent seed"),
        FixtureSupport::Supported(_)
    ));
    fixture.teardown().expect("first teardown");
    fixture.teardown().expect("idempotent teardown");
    assert!(!staging.exists());
    assert!(fixture.root.path().is_dir());
    assert_eq!(fs::read_dir(fixture.root.path()).expect("root remains").count(), 0);
}
