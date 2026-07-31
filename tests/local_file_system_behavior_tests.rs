// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Behavioral coverage for host and rooted local filesystem adapters.

use qubit_fs::{
    Checksum,
    ChecksumAlgorithm,
    CopyOptions,
    CopyConflictPolicy,
    CreateDirectoryOptions,
    DeleteOptions,
    FsErrorKind,
    ListOptions,
    MetadataPreservePolicy,
    Path,
    RenameOptions,
    ServerSidePreference,
    TempDirectoryOptions,
    TempFileOptions,
    UserMetadata,
    WriteOptions,
    WritePrecondition,
};
use qubit_fs_local::{
    LocalFileSystems,
    host_path_to_logical,
};

/// Creates a rooted adapter whose native root is removed with the fixture.
fn rooted_file_system() -> (tempfile::TempDir, qubit_fs::FileSystem) {
    let root = tempfile::tempdir().expect("test root must be created");
    let file_system = LocalFileSystems::rooted(root.path())
        .expect("rooted local filesystem must be opened");
    (root, file_system)
}

/// Unsupported provider-level metadata policies fail before filesystem side
/// effects are attempted.
#[test]
fn test_rooted_operations_reject_unrepresentable_option_metadata() {
    let (_root, file_system) = rooted_file_system();
    let metadata = UserMetadata::new()
        .with("key", "value")
        .expect("test metadata should be valid");

    let write = file_system
        .write_all(
            &path("/write"),
            b"payload",
            WriteOptions::default().with_user_metadata(metadata.clone()),
        )
        .expect_err("writer metadata must be rejected");
    assert_eq!(FsErrorKind::RequirementNotMet, write.error().kind());

    let directory = file_system
        .create_directory(
            &path("/directory"),
            CreateDirectoryOptions::default().with_user_metadata(metadata),
        )
        .expect_err("directory metadata must be rejected");
    assert_eq!(FsErrorKind::RequirementNotMet, directory.kind());

    let copy = file_system
        .copy(
            &path("/missing-source"),
            &path("/target"),
            CopyOptions {
                preserve_metadata: MetadataPreservePolicy::UserMetadata,
                ..CopyOptions::file()
            },
        )
        .expect_err("user metadata preservation must be rejected");
    assert_eq!(FsErrorKind::RequirementNotMet, copy.error().kind());

    let content_type = file_system
        .write_all(
            &path("/typed-write"),
            b"payload",
            WriteOptions {
                content_type: Some("text/plain".to_owned()),
                ..WriteOptions::default()
            },
        )
        .expect_err("content type must be rejected");
    assert_eq!(FsErrorKind::RequirementNotMet, content_type.error().kind());

    for options in [
        WriteOptions {
            precondition: WritePrecondition::IfAbsent,
            ..WriteOptions::default()
        },
        WriteOptions {
            checksum: Some(Checksum::new(ChecksumAlgorithm::Sha256, "00")),
            ..WriteOptions::default()
        },
    ] {
        let error = file_system
            .write_all(&path("/conditional-write"), b"payload", options)
            .expect_err("conditional local writes must be rejected");
        assert_eq!(FsErrorKind::RequirementNotMet, error.error().kind());
    }

    for options in [
        CopyOptions {
            continue_on_error: true,
            ..CopyOptions::file()
        },
        CopyOptions {
            server_side: ServerSidePreference::Require,
            ..CopyOptions::file()
        },
    ] {
        let error = file_system
            .copy(&path("/missing-source"), &path("/target"), options)
            .expect_err("unsupported copy option must be rejected");
        assert_eq!(FsErrorKind::RequirementNotMet, error.error().kind());
    }
}

/// Copy mode and conflict policy retain their public source-kind semantics.
#[test]
fn test_rooted_copy_maps_source_mode_and_type_conflict_skip() {
    let (_root, file_system) = rooted_file_system();
    let file = path("/file");
    let directory = path("/directory");
    file_system.write_all(&file, b"payload", WriteOptions::default())
        .expect("file fixture must be written");
    file_system.create_directory(&directory, CreateDirectoryOptions::default())
        .expect("directory fixture must be created");

    let tree_error = file_system.copy(&file, &path("/tree-target"), CopyOptions::tree())
        .expect_err("tree mode must reject a regular file source");
    assert_eq!(FsErrorKind::RequirementNotMet, tree_error.error().kind());
    let file_error = file_system.copy(&directory, &path("/file-target"), CopyOptions::file())
        .expect_err("file mode must reject a directory source");
    assert_eq!(FsErrorKind::RequirementNotMet, file_error.error().kind());
    file_system.copy(&directory, &path("/auto-target"), CopyOptions::default())
        .expect("auto mode must detect a directory source");

    let skipped = file_system.copy(
        &file,
        &directory,
        CopyOptions {
            conflict: CopyConflictPolicy::Skip,
            ..CopyOptions::file()
        },
    ).expect("skip policy must keep an incompatible destination");
    assert_eq!(1, skipped.stats().skipped);
    assert!(file_system.stat(&directory).expect("destination must remain").is_directory_like());
}

/// Copying with portable metadata preservation reports the policy actually
/// applied by the local adapter.
#[test]
fn test_rooted_copy_reports_portable_metadata_preservation() {
    let (_root, file_system) = rooted_file_system();
    let source = path("/source.txt");
    let target = path("/target.txt");
    file_system
        .write_all(&source, b"payload", WriteOptions::default())
        .expect("copy source must be written");

    let outcome = file_system
        .copy(
            &source,
            &target,
            CopyOptions {
                preserve_metadata: MetadataPreservePolicy::Portable,
                ..CopyOptions::file()
            },
        )
        .expect("portable metadata copy must satisfy the facade contract");

    assert_eq!(outcome.metadata(), MetadataPreservePolicy::Portable);
}

/// Rooted operation failures must preserve their public classifications.
#[test]
fn test_rooted_operations_map_missing_entries() {
    let (_root, file_system) = rooted_file_system();
    let missing = path("/missing");
    assert_eq!(
        FsErrorKind::NotFound,
        file_system
            .stat(&missing)
            .expect_err("missing stat must fail")
            .kind()
    );
    assert_eq!(
        FsErrorKind::NotFound,
        file_system
            .open_reader(&missing, Default::default())
            .expect_err("missing reader must fail")
            .kind()
    );
    assert_eq!(
        FsErrorKind::NotFound,
        file_system
            .list(&missing, ListOptions::default())
            .expect_err("missing listing must fail")
            .kind()
    );
    assert_eq!(
        FsErrorKind::NotFound,
        file_system
            .delete_file(&missing, DeleteOptions::default())
            .expect_err("missing file deletion must fail")
            .kind()
    );
    assert_eq!(
        FsErrorKind::NotFound,
        file_system
            .delete_directory(&missing, DeleteOptions::default())
            .expect_err("missing directory deletion must fail")
            .kind()
    );
    let target = path("/target");
    assert_eq!(
        FsErrorKind::NotFound,
        file_system
            .rename(&missing, &target, RenameOptions::default())
            .expect_err("missing rename must fail")
            .error()
            .kind()
    );
    assert_eq!(
        FsErrorKind::NotFound,
        file_system
            .copy(&missing, &target, CopyOptions::file())
            .expect_err("missing copy must fail")
            .error()
            .kind()
    );
    let writer = file_system
        .open_writer(&path("/absent-parent/output"), WriteOptions::default())
        .expect_err("writer without a parent must fail");
    assert_eq!(FsErrorKind::NotFound, writer.kind());
    let regular_file = path("/regular-file");
    file_system
        .write_all(&regular_file, b"file", WriteOptions::default())
        .expect("fixture file must be written");
    let create_directory = file_system
        .create_directory(&regular_file, CreateDirectoryOptions::default())
        .expect_err("a file cannot become a directory");
    assert_eq!(FsErrorKind::AlreadyExists, create_directory.kind());
    let temporary_file = file_system
        .create_temp_file(TempFileOptions {
            parent: Some(regular_file.clone()),
            ..TempFileOptions::default()
        })
        .expect_err("temporary file with a file parent must fail");
    assert_eq!(FsErrorKind::InvalidPath, temporary_file.kind());
    let temporary_directory = file_system
        .create_temp_directory(TempDirectoryOptions {
            parent: Some(regular_file),
            ..TempDirectoryOptions::default()
        })
        .expect_err("temporary directory with a file parent must fail");
    assert_eq!(FsErrorKind::InvalidPath, temporary_directory.kind());
}

/// Parses one absolute logical path used by the rooted adapter.
fn path(value: &str) -> Path {
    Path::parse(value).expect("test logical path must be valid")
}

/// Converts an isolated native fixture path into the host adapter namespace.
#[cfg(unix)]
fn host_path(root: &tempfile::TempDir, relative: &str) -> Path {
    let native = root.path().join(relative);
    host_path_to_logical(&native)
        .expect("test host logical path must be valid")
}

/// Host failures retain their operation-specific public error classifications.
#[cfg(unix)]
#[test]
fn test_host_operations_map_missing_native_entries() {
    let root = tempfile::tempdir().expect("host fixture root must be created");
    let file_system =
        LocalFileSystems::host().expect("host local filesystem must be opened");
    let missing = host_path(&root, "missing");

    let stat = file_system
        .stat(&missing)
        .expect_err("missing stat must fail");
    assert_eq!(FsErrorKind::NotFound, stat.kind());
    let reader = file_system
        .open_reader(&missing, Default::default())
        .expect_err("missing reader must fail");
    assert_eq!(FsErrorKind::NotFound, reader.kind());
    let listing = file_system
        .list(&missing, ListOptions::default())
        .expect_err("missing directory listing must fail");
    assert_eq!(FsErrorKind::NotFound, listing.kind());
    let deletion = file_system
        .delete_file(&missing, DeleteOptions::default())
        .expect_err("missing file deletion must fail");
    assert_eq!(FsErrorKind::NotFound, deletion.kind());
    let directory_deletion = file_system
        .delete_directory(&missing, DeleteOptions::default())
        .expect_err("missing directory deletion must fail");
    assert_eq!(FsErrorKind::NotFound, directory_deletion.kind());
    let renamed = host_path(&root, "renamed");
    let rename = file_system
        .rename(&missing, &renamed, RenameOptions::default())
        .expect_err("missing rename source must fail");
    assert_eq!(FsErrorKind::NotFound, rename.error().kind());
    let copy = file_system
        .copy(&missing, &renamed, CopyOptions::file())
        .expect_err("missing copy source must fail");
    assert_eq!(FsErrorKind::NotFound, copy.error().kind());

    let missing_child = host_path(&root, "absent-parent/output");
    let writer = file_system
        .open_writer(&missing_child, WriteOptions::default())
        .expect_err("writer without a parent must fail");
    assert_eq!(FsErrorKind::NotFound, writer.kind());

    let regular_file = root.path().join("regular-file");
    std::fs::write(&regular_file, b"file")
        .expect("regular fixture file must be written");
    let create_directory = file_system
        .create_directory(
            &host_path(&root, "regular-file"),
            CreateDirectoryOptions::default(),
        )
        .expect_err("a file cannot be created as a directory");
    assert_eq!(FsErrorKind::AlreadyExists, create_directory.kind());

    let file_parent = host_path(&root, "regular-file");
    let temporary_file = file_system
        .create_temp_file(TempFileOptions {
            parent: Some(file_parent.clone()),
            ..TempFileOptions::default()
        })
        .expect_err("temporary file with a file parent must fail");
    assert_eq!(FsErrorKind::AlreadyExists, temporary_file.kind());
    let temporary_directory = file_system
        .create_temp_directory(TempDirectoryOptions {
            parent: Some(file_parent),
            ..TempDirectoryOptions::default()
        })
        .expect_err("temporary directory with a file parent must fail");
    assert_eq!(FsErrorKind::AlreadyExists, temporary_directory.kind());
}

/// Host metadata preserves a symbolic link's own entry kind.
#[cfg(unix)]
#[test]
fn test_host_metadata_maps_symbolic_link_kind() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("host fixture root must exist");
    let file_system =
        LocalFileSystems::host().expect("host local filesystem must be opened");
    let referent = root.path().join("referent");
    let link = root.path().join("link");
    std::fs::write(&referent, b"payload").expect("referent should be written");
    symlink(&referent, &link).expect("symbolic link should be created");

    let metadata = file_system
        .stat(&host_path(&root, "link"))
        .expect("symbolic link metadata should be readable");
    assert_eq!(metadata.kind, qubit_fs::FileKind::Symlink);
}

/// Host metadata maps Unix-domain sockets to the adapter's generic local
/// entry kind.
#[cfg(unix)]
#[test]
fn test_host_metadata_maps_unix_socket_kind() {
    use std::os::unix::net::UnixListener;

    let root = tempfile::tempdir().expect("host fixture root must exist");
    let file_system =
        LocalFileSystems::host().expect("host local filesystem must be opened");
    let socket = root.path().join("socket");
    let _listener = UnixListener::bind(&socket)
        .expect("Unix-domain socket should be created");

    let metadata = file_system
        .stat(&host_path(&root, "socket"))
        .expect("socket metadata should be readable");
    assert_eq!(metadata.kind, qubit_fs::FileKind::Other("local".to_owned()));
}
