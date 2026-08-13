// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Behavioral coverage for host and rooted local filesystem adapters.

use qubit_fs::Checksum;
use qubit_fs::ChecksumAlgorithm;
use qubit_fs::CopyConflictPolicy;
use qubit_fs::CopyFailureState;
use qubit_fs::CopyOptions;
use qubit_fs::CreateDirectoryOptions;
use qubit_fs::DeleteOptions;
use qubit_fs::DurabilityRequirement;
use qubit_fs::FileKind;
use qubit_fs::FileSystem;
use qubit_fs::FsErrorKind;
use qubit_fs::ListOptions;
use qubit_fs::MetadataPreservePolicy;
use qubit_fs::Path;
use qubit_fs::PublicationMethod;
use qubit_fs::RenameFailureState;
use qubit_fs::RenameOptions;
use qubit_fs::ServerSidePreference;
use qubit_fs::SymlinkPolicy;
use qubit_fs::TempDirectoryOptions;
use qubit_fs::TempFileOptions;
use qubit_fs::UserMetadata;
use qubit_fs::WriteAbortOutcome;
use qubit_fs::WriteDisposition;
use qubit_fs::WriteFailureState;
use qubit_fs::WriteOptions;
use qubit_fs::WritePrecondition;
use qubit_fs::WriterState;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::host_path_to_logical;
use qubit_io::Output;

/// Creates a rooted adapter whose native root is removed with the fixture.
fn rooted_file_system() -> (tempfile::TempDir, FileSystem) {
    let root = tempfile::tempdir().expect("test root must be created");
    let file_system =
        LocalFileSystems::rooted(root.path()).expect("rooted local filesystem must be opened");
    (root, file_system)
}

/// Direct append cannot be rolled back, so abort must retain Published state.
#[test]
fn test_rooted_append_abort_reports_published_destination() {
    let (_root, file_system) = rooted_file_system();
    let target = path("/append-target");
    file_system
        .write_all(&target, b"base", WriteOptions::default())
        .expect("append fixture must be written");
    let mut writer = file_system
        .open_writer(
            &target,
            WriteOptions::default().with_disposition(WriteDisposition::Append),
        )
        .expect("append writer must open");
    Output::write_fully(&mut writer, b"-published").expect("append writer must accept bytes");

    let outcome = writer.abort().expect("append abort must flush");

    assert_eq!(WriteAbortOutcome::Published, outcome);
    assert_eq!(WriterState::Published, writer.state());
}

/// A terminal pre-publication conflict retains its confirmed destination state
/// for explicit facade cleanup without inventing a retryable native writer.
#[test]
fn test_host_commit_conflict_preserves_not_published_state() {
    let root = tempfile::tempdir().expect("host fixture root must be created");
    let target = root.path().join("target");
    let logical = host_path_to_logical(&target).expect("host target path must be logical");
    let file_system = LocalFileSystems::host().expect("host filesystem must open");
    let mut writer = file_system
        .open_writer(
            &logical,
            WriteOptions::default().with_disposition(WriteDisposition::CreateNew),
        )
        .expect("create-new writer must open before the conflict");
    Output::write_fully(&mut writer, b"staged").expect("writer must accept staged bytes");
    std::fs::write(&target, b"concurrent").expect("concurrent destination must be installed");

    let failure = writer
        .commit()
        .expect_err("concurrent destination must fail create-new commit");

    assert_eq!(WriteFailureState::NotPublished, failure.state(),);
    assert_eq!(WriterState::NotPublished, writer.state());
    assert_eq!(
        WriteAbortOutcome::NotPublished,
        writer.abort().expect("retained staging must abort"),
    );
}

/// Definite native cleanup failures retain the adapter session, so retrying
/// abort performs native cleanup again instead of reporting false success.
#[test]
fn test_host_abort_failure_retains_writer_for_retry() {
    let root = tempfile::tempdir().expect("host fixture root must be created");
    let target = root.path().join("target");
    let logical = host_path_to_logical(&target).expect("host target path must be logical");
    let file_system = LocalFileSystems::host().expect("host filesystem must open");
    let mut writer = file_system
        .open_writer(
            &logical,
            WriteOptions::default().with_disposition(WriteDisposition::CreateNew),
        )
        .expect("create-new writer must open");
    let staging = std::fs::read_dir(root.path())
        .expect("staging directory must be readable")
        .map(|entry| entry.expect("staging entry must be readable").path())
        .find(|path| path != &target)
        .expect("writer must create one staging entry");
    std::fs::remove_file(staging).expect("external actor must remove the staging entry");

    for _ in 0..2 {
        let error = writer
            .abort()
            .expect_err("missing staging cleanup must remain retryable");
        assert_eq!(FsErrorKind::NotFound, error.kind());
        assert_eq!(WriterState::Open, writer.state());
    }
}

/// Host listing maps the facade symlink option into recursive traversal.
#[cfg(unix)]
#[test]
fn test_host_list_symlink_policy_controls_directory_traversal() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("listing root must be created");
    let outside = tempfile::tempdir().expect("listing target must be created");
    std::fs::write(outside.path().join("child"), b"payload")
        .expect("listing child must be written");
    symlink(outside.path(), root.path().join("link")).expect("listing symlink must be created");
    let file_system = LocalFileSystems::host().expect("host filesystem must open");
    let logical_root = host_path_to_logical(root.path()).expect("listing root must be logical");

    let mut without_following_stream = file_system
        .list(
            &logical_root,
            ListOptions::default()
                .with_recursive(true)
                .with_symlink_policy(SymlinkPolicy::Reject),
        )
        .expect("non-following listing must open");
    let mut without_following = Vec::new();
    while let Some(entry) = without_following_stream
        .next_entry()
        .expect("non-following listing must succeed")
    {
        without_following.push(entry);
    }
    assert!(
        without_following
            .iter()
            .all(|entry| { !entry.path.as_str().ends_with("/link/child") })
    );

    let mut with_following_stream = file_system
        .list(
            &logical_root,
            ListOptions::default()
                .with_recursive(true)
                .with_symlink_policy(SymlinkPolicy::FollowWithinFileSystem),
        )
        .expect("following listing must open");
    let mut with_following = Vec::new();
    while let Some(entry) = with_following_stream
        .next_entry()
        .expect("following listing must succeed")
    {
        with_following.push(entry);
    }
    assert!(
        with_following
            .iter()
            .any(|entry| entry.path.as_str().ends_with("/link/child"))
    );
}

/// Host tree copy maps the facade symlink option into the native copy policy.
#[cfg(unix)]
#[test]
fn test_host_copy_symlink_policy_controls_directory_traversal() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("copy root must be created");
    let outside = tempfile::tempdir().expect("copy target must be created");
    std::fs::write(outside.path().join("child"), b"payload").expect("copy child must be written");
    let source = root.path().join("source");
    std::fs::create_dir(&source).expect("copy source must be created");
    symlink(outside.path(), source.join("link")).expect("copy symlink must be created");
    let file_system = LocalFileSystems::host().expect("host filesystem must open");
    let source = host_path_to_logical(&source).expect("copy source must be logical");

    let no_follow_target = root.path().join("no-follow");
    file_system
        .copy(
            &source,
            &host_path_to_logical(&no_follow_target).expect("copy target must be logical"),
            CopyOptions::tree().with_symlink_policy(SymlinkPolicy::Reject),
        )
        .expect("non-following tree copy must succeed");
    assert!(
        std::fs::symlink_metadata(no_follow_target.join("link"))
            .expect("copied link must be present")
            .file_type()
            .is_symlink()
    );

    let follow_target = root.path().join("follow");
    file_system
        .copy(
            &source,
            &host_path_to_logical(&follow_target).expect("copy target must be logical"),
            CopyOptions::tree().with_symlink_policy(SymlinkPolicy::FollowWithinFileSystem),
        )
        .expect("following tree copy must succeed");
    assert_eq!(
        std::fs::read(follow_target.join("link/child"))
            .expect("followed copy child must be present"),
        b"payload",
    );
}

/// Successful local writer commits expose their concrete publication method.
#[test]
fn test_rooted_writer_commit_reports_atomic_rename_publication() {
    let (_root, file_system) = rooted_file_system();
    let mut writer = file_system
        .open_writer(&path("/published"), WriteOptions::default())
        .expect("writer must open");
    Output::write_fully(&mut writer, b"payload").expect("writer must accept payload");

    let outcome = writer.commit().expect("writer commit must succeed");

    assert_eq!(PublicationMethod::AtomicRename, outcome.method());
    assert_eq!(Some(7_u64), outcome.bytes_written());
}

/// Required host rename succeeds when the provider advertises durable rename.
#[cfg(unix)]
#[test]
fn test_host_rename_supports_durable_operation() {
    let root = tempfile::tempdir().expect("rename root must be created");
    let source = root.path().join("source");
    let target = root.path().join("target");
    std::fs::write(&source, b"payload").expect("rename source must be written");
    let file_system = LocalFileSystems::host().expect("host filesystem must open");

    let outcome = file_system
        .rename(
            &host_path_to_logical(&source).expect("source path must be logical"),
            &host_path_to_logical(&target).expect("target path must be logical"),
            RenameOptions::default().with_durability(DurabilityRequirement::Required),
        )
        .expect("advertised durable rename must succeed");

    assert!(outcome.durable());
    assert!(!source.exists());
    assert!(target.exists());
}

/// Host metadata retains the native special-entry category in provider kind.
#[cfg(unix)]
#[test]
fn test_host_stat_preserves_character_device_kind() {
    let file_system = LocalFileSystems::host().expect("host filesystem must open");
    let path = host_path_to_logical(std::path::Path::new("/dev/null"))
        .expect("null-device path must be logical");

    let metadata = file_system
        .stat(&path)
        .expect("null-device metadata must be readable");

    assert_eq!(
        &FileKind::Other("local-char-device".to_owned()),
        metadata.kind(),
    );
}

/// Native I/O categories survive translation when a path component is not a
/// directory.
#[test]
fn test_stat_maps_not_a_directory_io_kind() {
    let root = tempfile::tempdir().expect("host fixture root must be created");
    let file_system = LocalFileSystems::host().expect("host local filesystem must be opened");
    let component = root.path().join("component");
    std::fs::write(&component, b"payload").expect("file fixture must be written");
    file_system
        .stat(&host_path_to_logical(&component).expect("component path must be logical"))
        .expect("file fixture metadata must be readable");

    let error = file_system
        .stat(&host_path_to_logical(&component.join("child")).expect("child path must be logical"))
        .expect_err("a child below a regular file must fail");
    assert_eq!(error.kind(), FsErrorKind::NotDirectory);
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
            CopyOptions::file().with_preserve_metadata(MetadataPreservePolicy::UserMetadata),
        )
        .expect_err("user metadata preservation must be rejected");
    assert_eq!(FsErrorKind::RequirementNotMet, copy.error().kind());

    let content_type = file_system
        .write_all(
            &path("/typed-write"),
            b"payload",
            WriteOptions::default().with_content_type(Some("text/plain".to_owned())),
        )
        .expect_err("content type must be rejected");
    assert_eq!(FsErrorKind::RequirementNotMet, content_type.error().kind());

    for options in [
        WriteOptions::default().with_precondition(WritePrecondition::IfAbsent),
        WriteOptions::default().with_checksum(Some(Checksum::new(ChecksumAlgorithm::Sha256, "00"))),
    ] {
        let error = file_system
            .write_all(&path("/conditional-write"), b"payload", options)
            .expect_err("conditional local writes must be rejected");
        assert_eq!(FsErrorKind::RequirementNotMet, error.error().kind());
    }

    for options in [
        CopyOptions::file().with_continue_on_error(true),
        CopyOptions::file().with_server_side(ServerSidePreference::Require),
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
    file_system
        .write_all(&file, b"payload", WriteOptions::default())
        .expect("file fixture must be written");
    file_system
        .create_directory(&directory, CreateDirectoryOptions::default())
        .expect("directory fixture must be created");

    let tree_error = file_system
        .copy(&file, &path("/tree-target"), CopyOptions::tree())
        .expect_err("tree mode must reject a regular file source");
    assert_eq!(FsErrorKind::RequirementNotMet, tree_error.error().kind());
    let file_error = file_system
        .copy(&directory, &path("/file-target"), CopyOptions::file())
        .expect_err("file mode must reject a directory source");
    assert_eq!(FsErrorKind::RequirementNotMet, file_error.error().kind());
    file_system
        .copy(&directory, &path("/auto-target"), CopyOptions::default())
        .expect("auto mode must detect a directory source");

    let skipped = file_system
        .copy(
            &file,
            &directory,
            CopyOptions::file().with_conflict(CopyConflictPolicy::Skip),
        )
        .expect("skip policy must keep an incompatible destination");
    assert_eq!(1, skipped.stats().skipped);
    assert!(
        file_system
            .stat(&directory)
            .expect("destination must remain")
            .is_directory_like()
    );
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
            CopyOptions::file().with_preserve_metadata(MetadataPreservePolicy::Portable),
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
    let rename = file_system
        .rename(&missing, &target, RenameOptions::default())
        .expect_err("missing rename must fail");
    assert_eq!(FsErrorKind::NotFound, rename.error().kind());
    assert_eq!(RenameFailureState::Unchanged, rename.state());
    let copy = file_system
        .copy(&missing, &target, CopyOptions::file())
        .expect_err("missing copy must fail");
    assert_eq!(FsErrorKind::NotFound, copy.error().kind());
    assert_eq!(CopyFailureState::Unchanged, copy.state());
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
    assert_eq!(FsErrorKind::Conflict, create_directory.kind());
    let temporary_file = file_system
        .create_temp_file(TempFileOptions::default().with_parent(Some(regular_file.clone())))
        .expect_err("temporary file with a file parent must fail");
    assert_eq!(FsErrorKind::NotDirectory, temporary_file.kind());
    let temporary_directory = file_system
        .create_temp_directory(TempDirectoryOptions::default().with_parent(Some(regular_file)))
        .expect_err("temporary directory with a file parent must fail");
    assert_eq!(FsErrorKind::NotDirectory, temporary_directory.kind());
}

/// Parses one absolute logical path used by the rooted adapter.
fn path(value: &str) -> Path {
    Path::parse(value).expect("test logical path must be valid")
}

/// Converts an isolated native fixture path into the host adapter namespace.
#[cfg(unix)]
fn host_path(root: &tempfile::TempDir, relative: &str) -> Path {
    let native = root.path().join(relative);
    host_path_to_logical(&native).expect("test host logical path must be valid")
}

/// Host failures retain their operation-specific public error classifications.
#[cfg(unix)]
#[test]
fn test_host_operations_map_missing_native_entries() {
    let root = tempfile::tempdir().expect("host fixture root must be created");
    let file_system = LocalFileSystems::host().expect("host local filesystem must be opened");
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
    assert_eq!(RenameFailureState::Unchanged, rename.state());
    let copy = file_system
        .copy(&missing, &renamed, CopyOptions::file())
        .expect_err("missing copy source must fail");
    assert_eq!(FsErrorKind::NotFound, copy.error().kind());
    assert_eq!(CopyFailureState::Unchanged, copy.state());

    let missing_child = host_path(&root, "absent-parent/output");
    let writer = file_system
        .open_writer(&missing_child, WriteOptions::default())
        .expect_err("writer without a parent must fail");
    assert_eq!(FsErrorKind::NotFound, writer.kind());

    let regular_file = root.path().join("regular-file");
    std::fs::write(&regular_file, b"file").expect("regular fixture file must be written");
    let create_directory = file_system
        .create_directory(
            &host_path(&root, "regular-file"),
            CreateDirectoryOptions::default(),
        )
        .expect_err("a file cannot be created as a directory");
    assert_eq!(FsErrorKind::Conflict, create_directory.kind());

    let file_parent = host_path(&root, "regular-file");
    let temporary_file = file_system
        .create_temp_file(TempFileOptions::default().with_parent(Some(file_parent.clone())))
        .expect_err("temporary file with a file parent must fail");
    assert_eq!(FsErrorKind::NotDirectory, temporary_file.kind());
    let temporary_directory = file_system
        .create_temp_directory(TempDirectoryOptions::default().with_parent(Some(file_parent)))
        .expect_err("temporary directory with a file parent must fail");
    assert_eq!(FsErrorKind::NotDirectory, temporary_directory.kind());
}

/// Host metadata preserves a symbolic link's own entry kind.
#[cfg(unix)]
#[test]
fn test_host_metadata_maps_symbolic_link_kind() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("host fixture root must exist");
    let file_system = LocalFileSystems::host().expect("host local filesystem must be opened");
    let referent = root.path().join("referent");
    let link = root.path().join("link");
    std::fs::write(&referent, b"payload").expect("referent should be written");
    symlink(&referent, &link).expect("symbolic link should be created");

    let metadata = file_system
        .stat(&host_path(&root, "link"))
        .expect("symbolic link metadata should be readable");
    assert_eq!(metadata.kind(), &FileKind::Symlink);
}

/// Host metadata maps Unix-domain sockets to the adapter's local socket kind.
#[cfg(unix)]
#[test]
fn test_host_metadata_maps_unix_socket_kind() {
    use std::os::unix::net::UnixListener;

    let root = tempfile::tempdir().expect("host fixture root must exist");
    let file_system = LocalFileSystems::host().expect("host local filesystem must be opened");
    let socket = root.path().join("socket");
    let _listener = UnixListener::bind(&socket).expect("Unix-domain socket should be created");

    let metadata = file_system
        .stat(&host_path(&root, "socket"))
        .expect("socket metadata should be readable");
    assert_eq!(metadata.kind(), &FileKind::Other("local-socket".to_owned()));
}
