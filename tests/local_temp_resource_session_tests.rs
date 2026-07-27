// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::{
    AtomicityRequirement, FileSystem, FileSystemExt, FsName, PersistOptions, TempFileOptions,
};
use qubit_fs_local::LocalFileSystem;

#[test]
fn test_local_temp_session_persists_file_content() {
    let fixture = tempfile::tempdir().expect("fixture directory should open");
    let parent =
        LocalFileSystem::path_from_native(fixture.path()).expect("fixture path should convert");
    let target =
        parent.child(&FsName::parse("persisted.bin").expect("target name should be valid"));
    let file_system = LocalFileSystem::host();
    let mut temporary = file_system
        .create_temp_file(TempFileOptions {
            parent: Some(parent),
            prefix: "session-".to_owned(),
            suffix: ".tmp".to_owned(),
        })
        .expect("temporary file should be created");

    temporary
        .resource()
        .write_all(b"persisted")
        .expect("temporary content should be written");
    temporary
        .persist(
            &target,
            PersistOptions {
                atomicity: AtomicityRequirement::Preferred,
                ..PersistOptions::default()
            },
        )
        .expect("temporary file should persist");

    assert_eq!(
        b"persisted",
        file_system
            .read_all(&target, 32)
            .expect("persisted file should be readable")
            .as_slice(),
    );
}

#[test]
fn test_local_temp_session_honors_overwrite_policy() {
    let fixture = tempfile::tempdir().expect("fixture directory should open");
    let parent =
        LocalFileSystem::path_from_native(fixture.path()).expect("fixture path should convert");
    let target = parent.child(&FsName::parse("existing.bin").expect("target name should be valid"));
    let file_system = LocalFileSystem::host();
    file_system
        .write_all(&target, b"existing")
        .expect("existing target should be created");
    let mut temporary = file_system
        .create_temp_file(TempFileOptions {
            parent: Some(parent),
            prefix: "overwrite-".to_owned(),
            suffix: ".tmp".to_owned(),
        })
        .expect("temporary file should be created");
    temporary
        .resource()
        .write_all(b"replacement")
        .expect("replacement content should be written");

    temporary
        .persist(
            &target,
            PersistOptions {
                atomicity: AtomicityRequirement::Preferred,
                ..PersistOptions::default()
            },
        )
        .expect_err("no-replace persistence should preserve the target");
    assert_eq!(
        b"existing",
        file_system
            .read_all(&target, 32)
            .expect("existing target should remain readable")
            .as_slice(),
    );

    temporary
        .persist(
            &target,
            PersistOptions {
                overwrite: true,
                atomicity: AtomicityRequirement::Preferred,
                ..PersistOptions::default()
            },
        )
        .expect("overwrite persistence should replace the target");
    assert_eq!(
        b"replacement",
        file_system
            .read_all(&target, 32)
            .expect("replacement target should be readable")
            .as_slice(),
    );
}
