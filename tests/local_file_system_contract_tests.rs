//! Stateful contract coverage for the rooted local adapter.

use qubit_fs::{
    CreateDirectoryOptions, FileSystem, FileSystemId, ListOptions, Path, TempDirectoryOptions,
    TempFileOptions, WriteOptions,
};
use qubit_fs_local::LocalFileSystems;
use qubit_fs_testkit::{
    FileSystemContractSuite, FileSystemFixture, FixtureError, FixtureResult, FixtureSupport,
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
        let fixture_dir = Path::parse("/fixture").expect("fixture path must be valid");
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
            return Path::parse("/")
                .map_err(|error| FixtureError::with_source("fixture path is invalid", error));
        }
        Path::parse(&format!("/fixture/{relative}"))
            .map_err(|error| FixtureError::with_source("fixture path is invalid", error))
    }

    fn list_prefix(&self, _: &Path, relative: &str) -> FixtureResult<String> {
        Ok(relative.to_owned())
    }

    fn seed_file(&self, relative: &str, bytes: &[u8]) -> FixtureResult<FixtureSupport<Path>> {
        let path = self.path(relative)?;
        self.file_system
            .write_all(&path, bytes, WriteOptions::default())
            .map_err(|failure| {
                FixtureError::new(format!("fixture seed write failed: {failure}"))
            })?;
        Ok(FixtureSupport::Supported(path))
    }

    fn read_file(&self, path: &Path) -> FixtureResult<FixtureSupport<Vec<u8>>> {
        self.file_system
            .read_all(path, Default::default(), 1024 * 1024)
            .map(FixtureSupport::Supported)
            .map_err(|error| FixtureError::with_source("fixture read failed", error))
    }
}

/// The rooted adapter satisfies the synchronous provider-neutral contract.
#[test]
fn test_rooted_local_adapter_passes_provider_contract() {
    let fixture = RootedFixture::new();
    FileSystemContractSuite::new(&fixture).assert_all();
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

/// Rooted temporary files honor the requested logical parent and name affixes.
#[test]
fn test_rooted_temp_file_applies_parent_and_affixes() {
    let fixture = RootedFixture::new();
    let parent = Path::parse("/fixture/temp-files").expect("temporary parent must be valid");
    fixture
        .file_system()
        .create_directory(&parent, CreateDirectoryOptions::default())
        .expect("temporary parent must be created");

    let temporary = fixture
        .file_system()
        .create_temp_file(TempFileOptions {
            parent: Some(parent),
            prefix: "upload-".to_owned(),
            suffix: ".part".to_owned(),
        })
        .expect("temporary file must be created");

    assert!(
        temporary
            .path()
            .as_str()
            .starts_with("/fixture/temp-files/upload-")
    );
    assert!(temporary.path().as_str().ends_with(".part"));
}

/// Rooted temporary directories honor the requested logical parent and affixes.
#[test]
fn test_rooted_temp_directory_applies_parent_and_affixes() {
    let fixture = RootedFixture::new();
    let parent = Path::parse("/fixture/temp-directories").expect("temporary parent must be valid");
    fixture
        .file_system()
        .create_directory(&parent, CreateDirectoryOptions::default())
        .expect("temporary parent must be created");

    let temporary = fixture
        .file_system()
        .create_temp_directory(TempDirectoryOptions {
            parent: Some(parent),
            prefix: "work-".to_owned(),
            suffix: ".tmp".to_owned(),
        })
        .expect("temporary directory must be created");

    assert!(
        temporary
            .path()
            .as_str()
            .starts_with("/fixture/temp-directories/work-")
    );
    assert!(temporary.path().as_str().ends_with(".tmp"));
}
