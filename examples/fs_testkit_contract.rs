// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Runs the provider-neutral filesystem contract suite against a local root.

use std::fs;
use std::io::ErrorKind;
use std::path::Path as NativePath;
use std::path::PathBuf;

use qubit_fs::FileSystem;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::path::Path;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_testkit::FileSystemContractSuite;
use qubit_fs_testkit::FileSystemFixture;
use qubit_fs_testkit::FixtureError;
use qubit_fs_testkit::FixtureResult;
use qubit_fs_testkit::FixtureSupport;
use qubit_local_files::path::LocalPaths;

/// Removes one native entry without following symbolic links.
fn remove_entry(path: &NativePath) -> FixtureResult<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(FixtureError::with_source(
                "example teardown metadata failed",
                error,
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        fs::remove_file(path).map_err(|error| {
            FixtureError::with_source("example teardown entry removal failed", error)
        })?;
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|error| {
        FixtureError::with_source("example teardown directory read failed", error)
    })? {
        let entry = entry.map_err(|error| {
            FixtureError::with_source("example teardown entry read failed", error)
        })?;
        remove_entry(&entry.path())?;
    }
    fs::remove_dir(path).map_err(|error| {
        FixtureError::with_source("example teardown directory removal failed", error)
    })
}

/// Removes all children of a native directory while retaining that directory.
fn clear_children(path: &NativePath) -> FixtureResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(FixtureError::new(
                "example teardown base is not a directory",
            ));
        }
        Err(error) => {
            return Err(FixtureError::with_source(
                "example teardown base metadata failed",
                error,
            ));
        }
    }
    for entry in fs::read_dir(path).map_err(|error| {
        FixtureError::with_source("example teardown directory read failed", error)
    })? {
        let entry = entry.map_err(|error| {
            FixtureError::with_source("example teardown entry read failed", error)
        })?;
        remove_entry(&entry.path())?;
    }
    Ok(())
}

/// Isolated descriptor-rooted fixture used by the example.
struct RootedFixture {
    root: tempfile::TempDir,
    file_system: FileSystem,
}

impl RootedFixture {
    /// Creates a temporary native root and opens the local facade over it.
    fn new() -> Self {
        let root = tempfile::tempdir().expect("example fixture root must be created");
        fs::create_dir(root.path().join("fixture"))
            .expect("example fixture namespace must be created");
        let id = FileSystemId::new("local-contract-example")
            .expect("example fixture identity must be valid");
        let file_system =
            LocalFileSystems::rooted_with_id(id, root.path(), LocalResourcePolicy::unbounded())
                .expect("example rooted filesystem must open");
        Self { root, file_system }
    }

    /// Converts a logical path into the same native path used by the facade.
    fn native_path(&self, path: &Path) -> FixtureResult<PathBuf> {
        match fs::symlink_metadata(self.root.path()) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(FixtureError::new("example fixture root is not a directory"));
            }
            Err(error) => {
                return Err(FixtureError::with_source(
                    "example fixture root metadata failed",
                    error,
                ));
            }
        }
        let virtual_path = LocalPaths::rooted()
            .from_canonical_components(path.components())
            .map_err(|error| FixtureError::with_source("example path conversion failed", error))?;
        let relative = virtual_path
            .strip_prefix(NativePath::new("/"))
            .map_err(|error| {
                FixtureError::with_source("example path is not namespace relative", error)
            })?;
        let native = self.root.path().join(relative);
        if !native.starts_with(self.root.path()) {
            return Err(FixtureError::new("example path escaped fixture root"));
        }
        let mut current = self.root.path().to_path_buf();
        for component in native
            .strip_prefix(self.root.path())
            .expect("native path was checked below fixture root")
            .components()
        {
            current.push(component.as_os_str());
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(FixtureError::new("example path contains a symbolic link"));
                }
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => break,
                Err(error) if error.kind() == ErrorKind::NotFound => break,
                Err(error) => {
                    return Err(FixtureError::with_source(
                        "example path containment check failed",
                        error,
                    ));
                }
            }
        }
        Ok(native)
    }

    /// Removes generated descendants while retaining the fixture namespace.
    fn teardown_entries(&self) -> FixtureResult<()> {
        clear_children(&self.root.path().join("fixture"))?;
        for entry in fs::read_dir(self.root.path())
            .map_err(|error| FixtureError::with_source("example root cleanup read failed", error))?
        {
            let entry = entry.map_err(|error| {
                FixtureError::with_source("example root cleanup entry failed", error)
            })?;
            if entry.file_name() != std::ffi::OsStr::new("fixture") {
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
            .map_err(|error| FixtureError::with_source("example fixture path is invalid", error))
    }

    fn seed_file(&self, relative: &str, bytes: &[u8]) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.path(relative)?;
        let native = self.native_path(&path)?;
        if let Some(parent) = native.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                FixtureError::with_source("example seed parent creation failed", error)
            })?;
        }
        fs::write(&native, bytes)
            .map_err(|error| FixtureError::with_source("example seed write failed", error))?;
        Ok(FixtureSupport::Supported(path))
    }

    fn seed_empty_directory(&self, relative: &str) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.path(relative)?;
        let native = self.native_path(&path)?;
        fs::create_dir_all(native).map_err(|error| {
            FixtureError::with_source("example directory creation failed", error)
        })?;
        Ok(FixtureSupport::Supported(path))
    }

    fn read_file(&self, path: &Path) -> FixtureResult<FixtureSupport<Vec<u8>>> {
        let native = self.native_path(path)?;
        fs::read(native)
            .map(FixtureSupport::Supported)
            .map_err(|error| FixtureError::with_source("example fixture read failed", error))
    }

    fn exists_out_of_band(&self, path: &Path) -> FixtureResult<FixtureSupport<bool>> {
        let native = self.native_path(path)?;
        match fs::symlink_metadata(native) {
            Ok(_) => Ok(FixtureSupport::Supported(true)),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Ok(FixtureSupport::Supported(false))
            }
            Err(error) => Err(FixtureError::with_source(
                "example existence observation failed",
                error,
            )),
        }
    }

    fn write_file_out_of_band(
        &self,
        path: &Path,
        bytes: &[u8],
    ) -> FixtureResult<FixtureSupport<()>> {
        let native = self.native_path(path)?;
        if let Some(parent) = native.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                FixtureError::with_source("example out-of-band parent creation failed", error)
            })?;
        }
        fs::write(native, bytes).map_err(|error| {
            FixtureError::with_source("example out-of-band write failed", error)
        })?;
        Ok(FixtureSupport::Supported(()))
    }

    fn teardown(&self) -> FixtureResult<FixtureSupport<()>> {
        self.teardown_entries()?;
        Ok(FixtureSupport::Supported(()))
    }
}

fn main() {
    let fixture = RootedFixture::new();
    let suite = FileSystemContractSuite::new(&fixture);
    suite.assert_all();
    println!("filesystem contract suite passed");
}
