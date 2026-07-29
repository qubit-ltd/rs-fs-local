//! Regression coverage for temporary-resource SPI recovery.

use qubit_fs::{
    CreateDirectoryOptions, DeleteOptions, FileSystemId, Path, PersistFailureState, PersistOptions,
    TempDirectoryOptions, TempFileOptions, TempResourceState, WriteOptions,
};
use qubit_fs_local::LocalFileSystems;

/// A confirmed destination conflict retains the native temporary file for
/// retry.
#[test]
fn test_temp_file_persist_conflict_retains_resource_for_retry() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id = FileSystemId::new("persist-recovery-root").expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let target = Path::parse("/published.txt").expect("target path must be valid");

    file_system
        .write_all(&target, b"existing", WriteOptions::default())
        .expect("conflicting target must be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions::default())
        .expect("temporary file must be created");

    let failure = temporary
        .persist(&target, PersistOptions::default())
        .expect_err("persist must report the existing destination");
    assert_eq!(failure.state(), PersistFailureState::NotPublished);
    assert_eq!(temporary.state(), TempResourceState::Owned);

    file_system
        .delete_file(&target, DeleteOptions::default())
        .expect("conflicting target must be removed");
    let outcome = temporary
        .persist(&target, PersistOptions::default())
        .expect("retained temporary file must be persistable after conflict removal");

    assert_eq!(outcome.target, target);
}

/// A confirmed destination conflict retains the native temporary directory for
/// retry.
#[test]
fn test_temp_directory_persist_conflict_retains_resource_for_retry() {
    let root = tempfile::tempdir().expect("test root must be created");
    let id =
        FileSystemId::new("persist-directory-recovery-root").expect("test identity must be valid");
    let file_system = LocalFileSystems::rooted_with_id(id, root.path())
        .expect("rooted filesystem must be created");
    let target = Path::parse("/published-directory").expect("target path must be valid");

    file_system
        .create_directory(&target, CreateDirectoryOptions::default())
        .expect("conflicting target directory must be created");
    let mut temporary = file_system
        .create_temp_directory(TempDirectoryOptions::default())
        .expect("temporary directory must be created");

    let failure = temporary
        .persist(&target, PersistOptions::default())
        .expect_err("persist must report the existing destination");
    assert_eq!(failure.state(), PersistFailureState::NotPublished);
    assert_eq!(temporary.state(), TempResourceState::Owned);

    file_system
        .delete_directory(&target, DeleteOptions::default())
        .expect("conflicting target directory must be removed");
    let outcome = temporary
        .persist(&target, PersistOptions::default())
        .expect("retained temporary directory must be persistable after conflict removal");

    assert_eq!(outcome.target, target);
}
