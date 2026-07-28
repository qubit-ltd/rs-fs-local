use qubit_fs::{FileSystemId, Path, PathSemantics, TempFileOptions};
use qubit_fs_local::LocalFileSystems;

#[test]
fn test_host_factory_returns_concrete_file_system() {
    let file_system = LocalFileSystems::host().expect("host filesystem should construct");
    assert_eq!(file_system.properties().info().provider_id(), "file");
    assert_eq!(
        file_system.properties().info().path_semantics(),
        PathSemantics::Hierarchical
    );
}

#[test]
fn test_rooted_with_id_preserves_explicit_identity() {
    let root = tempfile::tempdir().expect("root should exist");
    let id = FileSystemId::new("local-test-root").expect("filesystem id should be valid");
    let file_system = LocalFileSystems::rooted_with_id(id.clone(), root.path())
        .expect("rooted filesystem should construct");
    assert_eq!(file_system.properties().info().id(), &id);
}

/// Host temporary files honor the requested logical parent and name affixes.
#[cfg(unix)]
#[test]
fn test_host_temp_file_applies_parent_and_affixes() {
    let parent = tempfile::tempdir().expect("temporary parent should exist");
    let parent = Path::parse(
        parent
            .path()
            .to_str()
            .expect("test temporary path should be UTF-8"),
    )
    .expect("test temporary path should be logical");
    let file_system = LocalFileSystems::host().expect("host filesystem should construct");

    let temporary = file_system
        .create_temp_file(TempFileOptions {
            parent: Some(parent.clone()),
            prefix: "host-upload-".to_owned(),
            suffix: ".part".to_owned(),
        })
        .expect("temporary file should be created");

    assert!(
        temporary
            .path()
            .as_str()
            .starts_with(&format!("{}/host-upload-", parent.as_str()))
    );
    assert!(temporary.path().as_str().ends_with(".part"));
}
