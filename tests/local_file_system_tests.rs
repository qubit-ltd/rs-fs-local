// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

#[cfg(unix)]
use std::os::unix::{
    ffi::OsStringExt,
    fs::symlink,
    net::UnixListener,
};

use qubit_fs::{
    AchievedAtomicity,
    AtomicityRequirement,
    CopyConflictPolicy,
    CopyMethod,
    CopyMode,
    CopyOptions,
    CreateDirOptions,
    DeleteOptions,
    DirectoryStreamExt,
    FileKind,
    FileSystem,
    FileSystemCapabilities,
    FileSystemCapability,
    FileSystemExt,
    FileSystemLimits,
    FileSystemProperties,
    FsErrorKind,
    FsOperation,
    FsPath,
    ListOptions,
    PublicationMethod,
    ReadOptions,
    RenameOptions,
    WriteDisposition,
    WriteOptions,
};
use qubit_fs_local::LocalFileSystem;
use qubit_io::Output;

/// Converts a native test path into the canonical filesystem path form.
///
/// # Parameters
///
/// * `path` - Native path created by the test fixture.
///
/// # Returns
///
/// The lossless canonical filesystem path.
///
/// # Panics
///
/// Panics when the test fixture path cannot be represented as a valid
/// canonical filesystem path.
fn canonical_path(path: &Path) -> FsPath {
    LocalFileSystem::path_from_native(path).expect("decode native test path")
}

/// Counts Linux process descriptors that still reference the specified path.
///
/// # Parameters
///
/// * `path` - Native path whose open descriptors should be counted.
///
/// # Returns
///
/// The number of matching descriptors visible through `/proc/self/fd`.
///
/// # Panics
///
/// Panics when the process descriptor directory cannot be inspected.
#[cfg(target_os = "linux")]
fn open_descriptor_count(path: &Path) -> usize {
    std::fs::read_dir("/proc/self/fd")
        .expect("read process descriptor directory")
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter(|target| target == path)
        .count()
}

/// Confirms canonical percent escapes are decoded before native filesystem I/O.
#[test]
fn test_stat_supports_literal_percent_in_native_filename() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("100%ready.txt");
    std::fs::write(&file_path, b"payload").expect("write test file");
    let fs = LocalFileSystem::host();

    let metadata = fs
        .stat(&canonical_path(&file_path))
        .expect("stat percent-containing file");

    assert_eq!(metadata.kind, FileKind::File);
    assert_eq!(metadata.len, Some(7));
}

/// Confirms regular-file metadata is mapped into provider-neutral metadata.
#[test]
fn test_stat_maps_regular_file_metadata() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    std::fs::write(&file_path, b"payload").expect("write test file");
    let fs = LocalFileSystem::host();

    let metadata = fs
        .stat(&canonical_path(&file_path))
        .expect("stat regular file");

    assert_eq!(metadata.kind, FileKind::File);
    assert_eq!(metadata.len, Some(7));
}

/// Confirms directory metadata retains the directory kind.
#[test]
fn test_stat_maps_directory_metadata() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let fs = LocalFileSystem::host();

    let metadata = fs
        .stat(&canonical_path(temporary_directory.path()))
        .expect("stat directory");

    assert_eq!(metadata.kind, FileKind::Directory);
}

/// Confirms metadata lookup does not follow the final symbolic link on Unix.
#[cfg(unix)]
#[test]
fn test_stat_maps_final_symbolic_link_metadata() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let target_path = temporary_directory.path().join("target.txt");
    let link_path = temporary_directory.path().join("link.txt");
    std::fs::write(&target_path, b"payload").expect("write symlink target");
    symlink(&target_path, &link_path).expect("create symbolic link");
    let fs = LocalFileSystem::host();

    let metadata = fs
        .stat(&canonical_path(&link_path))
        .expect("stat symbolic link");

    assert_eq!(metadata.kind, FileKind::Symlink);
}

/// Confirms non-UTF-8 Unix filename bytes survive canonical path conversion.
#[cfg(unix)]
#[test]
fn test_stat_supports_non_utf8_native_filename() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let filename = std::ffi::OsString::from_vec(b"item-\xFF.txt".to_vec());
    let file_path = temporary_directory.path().join(filename);
    std::fs::write(&file_path, b"payload").expect("write non-UTF-8 test file");
    let fs = LocalFileSystem::host();

    let metadata = fs
        .stat(&canonical_path(&file_path))
        .expect("stat non-UTF-8 file");

    assert_eq!(metadata.kind, FileKind::File);
}

/// Confirms native special files map to the provider-neutral other kind.
#[cfg(unix)]
#[test]
fn test_stat_maps_native_special_file_metadata() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let socket_path = temporary_directory.path().join("service.socket");
    let _listener = UnixListener::bind(&socket_path).expect("bind Unix socket");
    let fs = LocalFileSystem::host();

    let metadata = fs
        .stat(&canonical_path(&socket_path))
        .expect("stat Unix socket");

    assert_eq!(metadata.kind, FileKind::Other("native-special".to_owned()));
}

/// Confirms host-local metadata lookup rejects relative native paths.
#[test]
fn test_stat_rejects_relative_native_path() {
    let fs = LocalFileSystem::host();
    let path = FsPath::parse("relative/item.txt").expect("parse relative path");

    let error = fs.stat(&path).expect_err("reject relative native path");

    assert_eq!(error.kind(), FsErrorKind::InvalidPath);
    assert_eq!(error.operation(), FsOperation::Stat);
    assert_eq!(error.path(), Some(&path));
    assert_eq!(error.provider(), Some("local-file"),);
}

/// Confirms host-local operations reject object-key literals that are not
/// canonical hierarchical paths.
#[test]
fn test_stat_rejects_noncanonical_hierarchical_paths() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    std::fs::create_dir(temporary_directory.path().join("nested"))
        .expect("create nested directory");
    let file_path = temporary_directory.path().join("value.txt");
    std::fs::write(&file_path, b"value").expect("write test file");
    let parent = canonical_path(temporary_directory.path());
    let fs = LocalFileSystem::host();
    let literal_paths = [
        format!("{}/./value.txt", parent.as_str()),
        format!("{}//value.txt", parent.as_str()),
        format!("{}/nested/../value.txt", parent.as_str()),
    ];

    for literal in literal_paths {
        let path = FsPath::parse_literal(&literal).expect("parse literal path");
        let error = fs
            .stat(&path)
            .expect_err("reject noncanonical hierarchical path");

        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::Stat, error.operation());
        assert_eq!(Some(&path), error.path());
        assert_eq!(Some("local-file"), error.provider());
    }
}

/// Confirms Windows-native separators cannot be smuggled through one canonical
/// path component.
#[cfg(windows)]
#[test]
fn test_stat_rejects_backslash_inside_canonical_component() {
    let fs = LocalFileSystem::host();
    let path = FsPath::parse("/C:/safe/..\\outside")
        .expect("parse canonical path text");

    let error = fs
        .stat(&path)
        .expect_err("reject embedded Windows separator");

    assert_eq!(error.kind(), FsErrorKind::InvalidPath);
    assert_eq!(error.operation(), FsOperation::Stat);
    assert_eq!(error.path(), Some(&path));
}

/// Confirms the host implementation advertises supported read and write
/// contracts.
#[test]
fn test_host_advertises_read_and_write_capabilities() {
    let fs = LocalFileSystem::host();

    assert_eq!(
        fs.capabilities(),
        FileSystemCapabilities::default()
            .with(FileSystemCapability::List)
            .with(FileSystemCapability::Read)
            .with(FileSystemCapability::Write)
            .with(FileSystemCapability::Append)
            .with(FileSystemCapability::CreateDirectory)
            .with(FileSystemCapability::Delete)
            .with(FileSystemCapability::RecursiveDelete)
            .with(FileSystemCapability::Rename)
            .with(FileSystemCapability::AtomicRename)
            .with(FileSystemCapability::AtomicReplace)
            .with(FileSystemCapability::Copy),
    );
}

/// Confirms direct and recursive listings preserve canonical paths, filtering,
/// kinds, and requested metadata.
#[test]
fn test_list_supports_recursive_prefix_and_metadata_options() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let nested = temporary_directory.path().join("nested");
    std::fs::create_dir(&nested).expect("create nested directory");
    std::fs::write(temporary_directory.path().join("alpha.txt"), b"alpha")
        .expect("write alpha file");
    std::fs::write(temporary_directory.path().join("beta.txt"), b"beta")
        .expect("write beta file");
    std::fs::write(nested.join("alpha-child.txt"), b"child")
        .expect("write nested alpha file");
    let root = canonical_path(temporary_directory.path());
    let fs = LocalFileSystem::host();
    let options = ListOptions {
        recursive: true,
        include_metadata: true,
        prefix: Some("alpha".to_owned()),
        ..ListOptions::default()
    };

    let entries = fs
        .list(&root, options)
        .expect("open recursive listing")
        .collect_entries(16)
        .expect("collect recursive listing");

    assert_eq!(2, entries.len());
    assert_eq!("alpha.txt", entries[0].name);
    assert_eq!(FileKind::File, entries[0].kind);
    assert_eq!(Some(5), entries[0].metadata.as_ref().and_then(|value| value.len));
    assert_eq!("alpha-child.txt", entries[1].name);
    assert_eq!(FileKind::File, entries[1].kind);
    assert_eq!(
        canonical_path(&nested.join("alpha-child.txt")),
        entries[1].path,
    );
}

/// Confirms directory creation honors parent and existing-directory policies.
#[test]
fn test_create_dir_honors_recursive_and_exists_ok_options() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let directory_path =
        temporary_directory.path().join("missing").join("nested");
    let path = canonical_path(&directory_path);
    let fs = LocalFileSystem::host();

    let error = fs
        .create_dir(&path, CreateDirOptions::default())
        .expect_err("missing parent should reject nonrecursive creation");
    assert_eq!(FsErrorKind::NotFound, error.kind());

    fs.create_dir(
        &path,
        CreateDirOptions {
            recursive: true,
            ..CreateDirOptions::default()
        },
    )
    .expect("create recursive directory");

    let error = fs
        .create_dir(&path, CreateDirOptions::default())
        .expect_err("existing directory should be rejected by default");
    assert_eq!(FsErrorKind::AlreadyExists, error.kind());
    fs.create_dir(
        &path,
        CreateDirOptions {
            exists_ok: true,
            ..CreateDirOptions::default()
        },
    )
    .expect("accept existing directory");
}

/// Confirms deletion distinguishes empty and recursive directory operations.
#[test]
fn test_delete_honors_recursive_and_missing_ok_options() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let directory_path = temporary_directory.path().join("tree");
    std::fs::create_dir(&directory_path).expect("create directory");
    std::fs::write(directory_path.join("value.txt"), b"value")
        .expect("write nested file");
    let path = canonical_path(&directory_path);
    let fs = LocalFileSystem::host();

    let error = fs
        .delete(&path, DeleteOptions::default())
        .expect_err("nonempty directory should reject nonrecursive deletion");
    assert_ne!(FsErrorKind::NotFound, error.kind());

    fs.delete(
        &path,
        DeleteOptions {
            recursive: true,
            ..DeleteOptions::default()
        },
    )
    .expect("delete directory tree");
    assert!(!directory_path.exists());

    fs.delete(
        &path,
        DeleteOptions {
            missing_ok: true,
            ..DeleteOptions::default()
        },
    )
    .expect("accept missing deletion target");
}

/// Confirms rename uses atomic no-replace semantics and explicit overwrite.
#[test]
fn test_rename_honors_destination_conflict_policy() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let source_path = temporary_directory.path().join("source.txt");
    let destination_path = temporary_directory.path().join("destination.txt");
    std::fs::write(&source_path, b"source").expect("write source");
    std::fs::write(&destination_path, b"destination")
        .expect("write destination");
    let source = canonical_path(&source_path);
    let destination = canonical_path(&destination_path);
    let fs = LocalFileSystem::host();

    let error = fs
        .rename(&source, &destination, RenameOptions::default())
        .expect_err("default rename should not replace a destination");
    assert_eq!(FsErrorKind::AlreadyExists, error.kind());
    assert_eq!(b"source", std::fs::read(&source_path).unwrap().as_slice());
    assert_eq!(
        b"destination",
        std::fs::read(&destination_path).unwrap().as_slice(),
    );

    let outcome = fs
        .rename(
            &source,
            &destination,
            RenameOptions {
                overwrite: true,
                ..RenameOptions::default()
            },
        )
        .expect("overwrite destination atomically");

    assert_eq!(AchievedAtomicity::Atomic, outcome.atomicity);
    assert_eq!(PublicationMethod::AtomicRename, outcome.method);
    assert!(!source_path.exists());
    assert_eq!(b"source", std::fs::read(&destination_path).unwrap().as_slice());
}

/// Confirms file and tree copy report local copy statistics and conflict
/// behavior.
#[test]
fn test_copy_supports_file_and_tree_modes() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let source_file = temporary_directory.path().join("source.txt");
    let destination_file = temporary_directory.path().join("destination.txt");
    std::fs::write(&source_file, b"source").expect("write source file");
    std::fs::write(&destination_file, b"destination")
        .expect("write destination file");
    let source = canonical_path(&source_file);
    let destination = canonical_path(&destination_file);
    let fs = LocalFileSystem::host();

    let error = fs
        .copy(&source, &destination, CopyOptions::file())
        .expect_err("default file copy should reject conflicts");
    assert_eq!(FsErrorKind::AlreadyExists, error.kind());

    let skipped = fs
        .copy(
            &source,
            &destination,
            CopyOptions {
                conflict: CopyConflictPolicy::Skip,
                ..CopyOptions::file()
            },
        )
        .expect("skip existing destination");
    assert_eq!(1, skipped.stats.skipped);
    assert_eq!(
        b"destination",
        std::fs::read(&destination_file).unwrap().as_slice(),
    );

    let copied = fs
        .copy(
            &source,
            &destination,
            CopyOptions {
                conflict: CopyConflictPolicy::Overwrite,
                ..CopyOptions::file()
            },
        )
        .expect("overwrite destination file");
    assert_eq!(CopyMethod::Local, copied.method);
    assert_eq!(1, copied.stats.files);
    assert_eq!(6, copied.stats.bytes);
    assert_eq!(1, copied.stats.overwritten);
    assert_eq!(b"source", std::fs::read(&destination_file).unwrap().as_slice());

    let source_tree = temporary_directory.path().join("source-tree");
    let destination_tree = temporary_directory.path().join("destination-tree");
    std::fs::create_dir(&source_tree).expect("create source tree");
    std::fs::write(source_tree.join("value.txt"), b"tree")
        .expect("write tree file");
    let tree_outcome = fs
        .copy(
            &canonical_path(&source_tree),
            &canonical_path(&destination_tree),
            CopyOptions {
                mode: CopyMode::Tree,
                ..CopyOptions::tree()
            },
        )
        .expect("copy directory tree");

    assert_eq!(CopyMethod::Local, tree_outcome.method);
    assert_eq!(1, tree_outcome.stats.files);
    assert_eq!(1, tree_outcome.stats.directories);
    assert_eq!(4, tree_outcome.stats.bytes);
    assert_eq!(
        b"tree",
        std::fs::read(destination_tree.join("value.txt"))
            .unwrap()
            .as_slice(),
    );
}

/// Confirms the default preferred write uses atomic whole-file publication.
#[test]
fn test_write_all_atomically_replaces_local_file() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    std::fs::write(&file_path, b"old").expect("write old contents");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();

    let outcome = fs.write_all(&path, b"replacement").expect("write file");

    assert_eq!(
        b"replacement",
        std::fs::read(&file_path).unwrap().as_slice()
    );
    assert_eq!(Some(11), outcome.bytes_written);
    assert_eq!(AchievedAtomicity::Atomic, outcome.atomicity);
    assert_eq!(PublicationMethod::AtomicRename, outcome.method);
}

/// Confirms failed atomic publication retains its session for explicit abort.
#[test]
fn test_atomic_commit_failure_retains_session_for_abort() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    std::fs::write(&file_path, b"old").expect("write old contents");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();
    let mut writer = fs
        .open_writer(&path, WriteOptions::default())
        .expect("open atomic writer");
    writer
        .write_fully(b"replacement")
        .expect("stage replacement");
    std::fs::remove_file(&file_path).expect("remove destination before commit");

    let error = writer
        .commit()
        .expect_err("missing destination should reject commit");

    assert_eq!(FsErrorKind::NotFound, error.kind());
    writer
        .abort()
        .expect("failed atomic session should remain available for abort");
    assert_eq!(
        0,
        std::fs::read_dir(temporary_directory.path())
            .expect("read temporary directory")
            .count(),
        "explicit abort must remove retained atomic staging",
    );
}

/// Confirms explicitly non-atomic replacement writes directly to the target.
#[test]
fn test_open_writer_supports_direct_replacement() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();
    let options = WriteOptions {
        atomicity: AtomicityRequirement::NotRequired,
        ..WriteOptions::default()
    };

    let mut writer =
        fs.open_writer(&path, options).expect("open direct writer");
    writer
        .write_fully(b"direct")
        .expect("write direct contents");
    let outcome = writer.commit().expect("commit direct write");

    assert_eq!(b"direct", std::fs::read(&file_path).unwrap().as_slice());
    assert_eq!(Some(6), outcome.bytes_written);
    assert_eq!(AchievedAtomicity::NonAtomic, outcome.atomicity);
    assert_eq!(PublicationMethod::Direct, outcome.method);
}

/// Confirms a committed direct writer releases its native descriptor even
/// while the provider-neutral writer value remains alive.
#[cfg(target_os = "linux")]
#[test]
fn test_direct_commit_releases_native_descriptor() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();
    let options = WriteOptions {
        atomicity: AtomicityRequirement::NotRequired,
        ..WriteOptions::default()
    };
    let mut writer =
        fs.open_writer(&path, options).expect("open direct writer");
    writer
        .write_fully(b"direct")
        .expect("write direct contents");
    assert_eq!(1, open_descriptor_count(&file_path));

    writer.commit().expect("commit direct write");

    assert_eq!(0, open_descriptor_count(&file_path));
}

/// Confirms append sessions preserve existing bytes and report direct output.
#[test]
fn test_open_writer_appends_to_existing_file() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    std::fs::write(&file_path, b"first").expect("write initial contents");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();
    let options = WriteOptions {
        disposition: WriteDisposition::Append,
        atomicity: AtomicityRequirement::NotRequired,
        ..WriteOptions::default()
    };

    let mut writer =
        fs.open_writer(&path, options).expect("open append writer");
    writer.write_fully(b"-second").expect("append contents");
    let outcome = writer.commit().expect("commit append");

    assert_eq!(
        b"first-second",
        std::fs::read(&file_path).unwrap().as_slice()
    );
    assert_eq!(Some(7), outcome.bytes_written);
    assert_eq!(PublicationMethod::Direct, outcome.method);
}

/// Confirms create-new refuses to truncate an existing destination.
#[test]
fn test_open_writer_create_new_preserves_existing_file() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    std::fs::write(&file_path, b"original").expect("write original contents");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();
    let options = WriteOptions {
        disposition: WriteDisposition::CreateNew,
        atomicity: AtomicityRequirement::NotRequired,
        ..WriteOptions::default()
    };

    let error = fs
        .open_writer(&path, options)
        .expect_err("existing destination should be rejected");

    assert_eq!(FsErrorKind::AlreadyExists, error.kind());
    assert_eq!(b"original", std::fs::read(&file_path).unwrap().as_slice());
}

/// Confirms parent creation follows the provider-neutral write option.
#[test]
fn test_open_writer_respects_create_parent() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let parent = temporary_directory.path().join("missing").join("nested");
    let file_path = parent.join("item.txt");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();

    let error = fs
        .open_writer(&path, WriteOptions::default())
        .expect_err("missing parent should fail");
    assert_eq!(FsErrorKind::NotFound, error.kind());
    assert!(!parent.exists());

    let options = WriteOptions {
        create_parent: true,
        ..WriteOptions::default()
    };
    let mut writer = fs.open_writer(&path, options).expect("create parents");
    writer.write_fully(b"payload").expect("write payload");
    writer.commit().expect("commit payload");
    assert_eq!(b"payload", std::fs::read(&file_path).unwrap().as_slice());
}

/// Confirms required atomic create-new is rejected before filesystem effects.
#[test]
fn test_open_writer_rejects_required_atomic_create_new_before_io() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("missing").join("item.txt");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();
    let options = WriteOptions {
        create_parent: true,
        disposition: WriteDisposition::CreateNew,
        atomicity: AtomicityRequirement::Required,
        ..WriteOptions::default()
    };

    let error = fs
        .open_writer(&path, options)
        .expect_err("atomic create-new is unsupported");

    assert_eq!(FsErrorKind::RequirementNotMet, error.kind());
    assert_eq!(FsOperation::OpenWriter, error.operation());
    assert!(!file_path.parent().unwrap().exists());
}

/// Confirms unsupported content metadata is rejected before opening a file.
#[test]
fn test_open_writer_rejects_content_type_before_io() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("missing").join("item.txt");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();
    let options = WriteOptions {
        create_parent: true,
        content_type: Some("text/plain".to_owned()),
        ..WriteOptions::default()
    };

    let error = fs
        .open_writer(&path, options)
        .expect_err("content type is unsupported");

    assert_eq!(FsErrorKind::InvalidOptions, error.kind());
    assert!(!file_path.parent().unwrap().exists());
}

/// Confirms local limits are represented explicitly instead of omitted.
#[test]
fn test_host_reports_unknown_limits() {
    let fs = LocalFileSystem::host();

    assert_eq!(fs.limits(), &FileSystemLimits::unknown());
}

/// Confirms the filesystem extension reads a complete local file.
#[test]
fn test_read_all_reads_regular_file() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    std::fs::write(&file_path, b"payload").expect("write test file");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();

    let content = fs.read_all(&path, 64).expect("read complete local file");

    assert_eq!(content, b"payload");
}

/// Confirms an opened reader is bound to the requested filesystem location.
#[test]
fn test_open_reader_binds_file_location() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("item.txt");
    std::fs::write(&file_path, b"payload").expect("write test file");
    let path = canonical_path(&file_path);
    let fs = LocalFileSystem::host();

    let reader = fs
        .open_reader(&path, ReadOptions::default())
        .expect("open local reader");

    assert_eq!(reader.info().location().path(), &path);
    assert_eq!(reader.info().location().file_system_id(), fs.info().id(),);
}

/// Confirms unsupported range semantics fail before native file lookup.
#[test]
fn test_open_reader_preflights_range_options_before_io() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let path = canonical_path(&temporary_directory.path().join("missing.txt"));
    let fs = LocalFileSystem::host();
    let options = ReadOptions {
        offset: Some(1),
        ..ReadOptions::default()
    };

    let error = fs
        .open_reader(&path, options)
        .expect_err("reject unsupported range read");

    assert_eq!(error.kind(), FsErrorKind::RequirementNotMet);
    assert_eq!(error.operation(), FsOperation::OpenReader);
    assert_eq!(
        error.required_capability(),
        Some(FileSystemCapability::RangeRead),
    );
    assert_eq!(error.path(), Some(&path));
    assert_eq!(error.provider(), Some("local-file"),);
}

/// Confirms native I/O failures retain operation, path, and provider context.
#[test]
fn test_stat_maps_missing_path_with_context() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let path = canonical_path(&temporary_directory.path().join("missing.txt"));
    let fs = LocalFileSystem::host();

    let error = fs.stat(&path).expect_err("report missing file");

    assert_eq!(error.kind(), FsErrorKind::NotFound);
    assert_eq!(error.operation(), FsOperation::Stat);
    assert_eq!(error.path(), Some(&path));
    assert_eq!(error.provider(), Some("local-file"),);
}
