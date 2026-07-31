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
    CopyConflictPolicy,
    CopyOptions,
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
    WriteDisposition,
    WriteOptions,
    WritePrecondition,
};
use qubit_fs_local::LocalFileSystems;
use qubit_io::Output;

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

/// Copy failures retain native staging cleanup diagnostics as a typed source.
#[cfg(coverage)]
#[test]
fn test_host_copy_failure_retains_cleanup_diagnostics() {
    use std::error::Error;
    use std::process::Command;

    const TEST_NAME: &str =
        "test_host_copy_failure_retains_cleanup_diagnostics";
    const FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
    if std::env::var_os(FAULT_ENV).is_none() {
        let executable =
            std::env::current_exe().expect("test executable must be available");
        let status = Command::new(executable)
            .arg("--exact")
            .arg(TEST_NAME)
            .arg("--nocapture")
            .env(FAULT_ENV, "copy-staging-copy-cleanup")
            .status()
            .expect("coverage fault child must launch");
        assert!(status.success(), "coverage fault child must pass");
        return;
    }

    let root = tempfile::tempdir().expect("copy fixture root must be created");
    let source = root.path().join("source");
    std::fs::create_dir(&source)
        .expect("copy source directory must be created");
    std::fs::write(source.join("payload"), b"payload")
        .expect("copy source must be written");
    let file_system =
        LocalFileSystems::host().expect("host filesystem must be opened");
    let failure = file_system
        .copy(
            &host_path(&root, "source"),
            &host_path(&root, "target"),
            CopyOptions::tree(),
        )
        .expect_err("staging and cleanup faults must fail");
    let source = Error::source(failure.error())
        .expect("copy failure must retain a native source");
    let native = source
        .downcast_ref::<qubit_local_files::LocalCopyFailure>()
        .expect("copy failure source must retain LocalCopyFailure");

    assert!(native.staging_path().is_some());
    assert!(native.cleanup_error().is_some());
    assert_eq!(0, native.partial_stats().files());
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
    assert_eq!(FsErrorKind::InvalidOptions, temporary_file.kind());
    let temporary_directory = file_system
        .create_temp_directory(TempDirectoryOptions {
            parent: Some(regular_file),
            ..TempDirectoryOptions::default()
        })
        .expect_err("temporary directory with a file parent must fail");
    assert_eq!(FsErrorKind::InvalidOptions, temporary_directory.kind());

    let invalid_prefix = file_system
        .create_temp_file(TempFileOptions {
            prefix: "invalid/prefix".to_owned(),
            ..TempFileOptions::default()
        })
        .expect_err("a temp prefix containing a separator must fail");
    assert_eq!(FsErrorKind::InvalidOptions, invalid_prefix.kind());
}

/// Parses one absolute logical path used by the rooted adapter.
fn path(value: &str) -> Path {
    Path::parse(value).expect("test logical path must be valid")
}

/// Converts an isolated native fixture path into the host adapter namespace.
#[cfg(unix)]
fn host_path(root: &tempfile::TempDir, relative: &str) -> Path {
    let native = root.path().join(relative);
    Path::parse(
        native
            .to_str()
            .expect("test native path must be valid UTF-8"),
    )
    .expect("test host logical path must be valid")
}

/// Rooted writes support create-parent, create-new, replacement, append, and
/// terminal writer sessions.
#[test]
fn test_rooted_writer_supports_publication_modes_and_terminal_sessions() {
    let (_root, file_system) = rooted_file_system();
    let target = path("/nested/output.txt");
    file_system
        .write_all(
            &target,
            b"first",
            WriteOptions {
                create_parent: true,
                ..WriteOptions::default()
            },
        )
        .expect("create-parent write must succeed");

    let error = file_system
        .write_all(
            &target,
            b"again",
            WriteOptions {
                disposition: WriteDisposition::CreateNew,
                ..WriteOptions::default()
            },
        )
        .expect_err("create-new write must reject an existing target");
    assert_eq!(error.error().kind(), FsErrorKind::AlreadyExists);

    file_system
        .write_all(&target, b"replace", WriteOptions::default())
        .expect("replacement write must succeed");
    file_system
        .write_all(
            &target,
            b"+append",
            WriteOptions {
                disposition: WriteDisposition::Append,
                ..WriteOptions::default()
            },
        )
        .expect("append write must succeed");
    assert_eq!(
        file_system
            .read_all(&target, Default::default(), 1024)
            .expect("written bytes must be readable"),
        b"replace+append"
    );

    let terminal = path("/terminal.txt");
    let mut writer = file_system
        .open_writer(&terminal, WriteOptions::default())
        .expect("writer must open");
    Output::write_fully(&mut writer, b"terminal")
        .expect("writer must accept bytes");
    writer.commit().expect("first commit must publish");
    let error = writer
        .commit()
        .expect_err("second commit must reject a terminal session");
    assert_eq!(error.error().kind(), FsErrorKind::InvalidState);
    let error = writer
        .abort()
        .expect_err("published writer must reject a later abort");
    assert_eq!(error.kind(), FsErrorKind::InvalidState);

    let mut aborted = file_system
        .open_writer(&path("/aborted.txt"), WriteOptions::default())
        .expect("writer must open for abort");
    aborted.abort().expect("open writer must abort");
    assert!(
        !file_system
            .exists(&path("/aborted.txt"))
            .expect("existence probe must succeed")
    );
}

/// Rooted create, delete, copy, and rename operations preserve their explicit
/// conflict and recursive policies.
#[test]
fn test_rooted_mutation_operations_cover_conflict_and_recursive_policies() {
    let (_root, file_system) = rooted_file_system();
    let tree = path("/tree/child");
    file_system
        .create_directory(
            &tree,
            CreateDirectoryOptions {
                recursive: true,
                ..CreateDirectoryOptions::default()
            },
        )
        .expect("recursive directory creation must succeed");
    let source = path("/tree/child/source.txt");
    file_system
        .write_all(&source, b"source", WriteOptions::default())
        .expect("source file must be written");

    let created_parent_target = path("/created/parent/target.txt");
    file_system
        .copy(
            &source,
            &created_parent_target,
            CopyOptions {
                create_parent: true,
                ..CopyOptions::file()
            },
        )
        .expect("copy must create missing target parents");
    assert_eq!(
        file_system
            .read_all(&created_parent_target, Default::default(), 1024)
            .expect("copied bytes must be readable"),
        b"source"
    );

    let target = path("/tree/child/target.txt");
    file_system
        .copy(&source, &target, CopyOptions::file())
        .expect("file copy must succeed");
    let error = file_system
        .copy(&source, &target, CopyOptions::file())
        .expect_err("default conflict policy must preserve destination");
    assert_eq!(error.error().kind(), FsErrorKind::AlreadyExists);
    let skipped = file_system
        .copy(
            &source,
            &target,
            CopyOptions {
                conflict: CopyConflictPolicy::Skip,
                ..CopyOptions::file()
            },
        )
        .expect("skip conflict policy must complete");
    assert_eq!(skipped.stats().skipped, 1);
    file_system
        .copy(
            &source,
            &target,
            CopyOptions {
                conflict: CopyConflictPolicy::Overwrite,
                ..CopyOptions::file()
            },
        )
        .expect("overwrite conflict policy must replace destination");

    let renamed = path("/tree/child/renamed.txt");
    file_system
        .rename(&target, &renamed, RenameOptions::default())
        .expect("rename must succeed");
    let destination = path("/tree/child/destination.txt");
    file_system
        .write_all(&destination, b"occupied", WriteOptions::default())
        .expect("rename destination must be occupied");
    let error = file_system
        .rename(&renamed, &destination, RenameOptions::default())
        .expect_err("rename must reject an occupied destination by default");
    assert_eq!(error.error().kind(), FsErrorKind::AlreadyExists);
    file_system
        .rename(
            &renamed,
            &destination,
            RenameOptions {
                overwrite: true,
                ..RenameOptions::default()
            },
        )
        .expect("overwrite rename must replace destination");

    let error = file_system
        .delete_directory(&path("/tree"), DeleteOptions::default())
        .expect_err("non-recursive deletion must reject a nonempty directory");
    assert_eq!(error.kind(), FsErrorKind::Conflict);
    file_system
        .delete_directory(
            &path("/tree"),
            DeleteOptions {
                recursive: true,
                ..DeleteOptions::default()
            },
        )
        .expect("recursive deletion must remove the directory tree");
    file_system
        .delete_file(
            &path("/tree/missing.txt"),
            DeleteOptions {
                missing_ok: true,
                ..DeleteOptions::default()
            },
        )
        .expect("missing-ok deletion must be successful");
}

/// Host operations apply the same policies while preserving absolute native
/// namespace translation.
#[cfg(unix)]
#[test]
fn test_host_mutation_operations_cover_native_path_translation() {
    let root = tempfile::tempdir().expect("host fixture root must be created");
    let file_system =
        LocalFileSystems::host().expect("host local filesystem must be opened");
    let directory = host_path(&root, "nested/child");
    let source = host_path(&root, "nested/child/source.txt");
    let target = host_path(&root, "nested/child/target.txt");
    let renamed = host_path(&root, "nested/child/renamed.txt");

    file_system
        .create_directory(
            &directory,
            CreateDirectoryOptions {
                recursive: true,
                ..CreateDirectoryOptions::default()
            },
        )
        .expect("host recursive directory creation must succeed");
    file_system
        .write_all(&source, b"host source", WriteOptions::default())
        .expect("host source write must succeed");
    file_system
        .copy(&source, &target, CopyOptions::file())
        .expect("host file copy must succeed");
    file_system
        .rename(&target, &renamed, RenameOptions::default())
        .expect("host rename must succeed");
    assert_eq!(
        file_system
            .read_all(&renamed, Default::default(), 1024)
            .expect("host renamed file must be readable"),
        b"host source"
    );
    file_system
        .delete_file(&renamed, DeleteOptions::default())
        .expect("host file deletion must succeed");
    file_system
        .delete_directory(
            &host_path(&root, "nested"),
            DeleteOptions {
                recursive: true,
                ..DeleteOptions::default()
            },
        )
        .expect("host recursive directory deletion must succeed");
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

    let file_child = host_path(&root, "regular-file/child");
    let file_child_writer = file_system
        .open_writer(&file_child, WriteOptions::default())
        .expect_err("a regular file cannot be used as a parent directory");
    assert_eq!(FsErrorKind::NotDirectory, file_child_writer.kind());

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

    let invalid_prefix = file_system
        .create_temp_file(TempFileOptions {
            prefix: "invalid/prefix".to_owned(),
            ..TempFileOptions::default()
        })
        .expect_err("a host temp prefix containing a separator must fail");
    assert_eq!(FsErrorKind::InvalidOptions, invalid_prefix.kind());
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
