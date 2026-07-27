// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::{
    AchievedAtomicity,
    AtomicityRequirement,
    CopyConflictPolicy,
    CopyMethod,
    CopyOptions,
    CreateDirOptions,
    DeleteOptions,
    DirectoryStreamExt,
    FileKind,
    FileSystem,
    FileSystemCapabilities,
    FileSystemCapability,
    FileSystemExt,
    FileSystemId,
    FileSystemProperties,
    FsErrorKind,
    FsOperation,
    FsPath,
    ListOptions,
    PublicationMethod,
    RenameOptions,
    WriteDisposition,
    WriteOptions,
};
use qubit_fs_local::RootedLocalFileSystem;
use qubit_io::Output;

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
fn open_descriptor_count(path: &std::path::Path) -> usize {
    std::fs::read_dir("/proc/self/fd")
        .expect("read process descriptor directory")
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter(|target| target == path)
        .count()
}

/// Opens a rooted filesystem with a stable test identity.
#[cfg(unix)]
fn open_rooted_file_system(
    id: &str,
    path: &std::path::Path,
) -> RootedLocalFileSystem {
    let id =
        FileSystemId::new(id).expect("the test filesystem ID should validate");
    RootedLocalFileSystem::open(id, path).expect("the root should open")
}

/// Verifies rooted writes and reads stay beneath the opened directory.
#[cfg(unix)]
#[test]
fn test_rooted_local_file_system_round_trips_content() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system =
        open_rooted_file_system("rooted-round-trip", directory.path());
    let path = FsPath::parse("/value.txt").expect("the path should parse");

    file_system
        .write_all(&path, b"rooted")
        .expect("the rooted write should succeed");
    assert_eq!(
        b"rooted",
        file_system
            .read_all(&path, 32)
            .expect("the rooted read should succeed")
            .as_slice(),
    );
}

/// Verifies rooted filesystems advertise durable atomic replacement.
#[cfg(unix)]
#[test]
fn test_rooted_local_file_system_advertises_atomic_replace() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system =
        open_rooted_file_system("rooted-capabilities", directory.path());

    assert_eq!(
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
        file_system.capabilities(),
    );
}

/// Verifies rooted namespace management stays beneath the opened authority.
#[cfg(unix)]
#[test]
fn test_rooted_namespace_management_operations() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system =
        open_rooted_file_system("rooted-namespace", directory.path());
    let nested = FsPath::parse("/tree/nested").expect("the path should parse");
    file_system
        .create_dir(
            &nested,
            CreateDirOptions {
                recursive: true,
                ..CreateDirOptions::default()
            },
        )
        .expect("the nested directory should be created");
    file_system
        .write_all(
            &FsPath::parse("/tree/nested/value.txt")
                .expect("the path should parse"),
            b"value",
        )
        .expect("the fixture should be written");

    let entries = file_system
        .list(
            &FsPath::parse("/tree").expect("the path should parse"),
            ListOptions {
                recursive: true,
                include_metadata: true,
                prefix: Some("nested/value.txt".to_owned()),
                ..ListOptions::default()
            },
        )
        .expect("the listing should open")
        .collect_entries(8)
        .expect("the listing should complete");
    assert_eq!(1, entries.len());
    assert_eq!("/tree/nested/value.txt", entries[0].path.as_str());
    assert_eq!(Some(5), entries[0].metadata.as_ref().and_then(|m| m.len));

    let source = FsPath::parse("/tree/nested/value.txt")
        .expect("the source should parse");
    let destination = FsPath::parse("/tree/nested/moved.txt")
        .expect("the destination should parse");
    let outcome = file_system
        .rename(&source, &destination, RenameOptions::default())
        .expect("the file should be renamed");
    assert_eq!(AchievedAtomicity::Atomic, outcome.atomicity);
    assert_eq!(PublicationMethod::AtomicRename, outcome.method);

    file_system
        .delete(
            &FsPath::parse("/tree").expect("the path should parse"),
            DeleteOptions {
                recursive: true,
                ..DeleteOptions::default()
            },
        )
        .expect("the tree should be removed");
    assert!(!directory.path().join("tree").exists());
}

/// Verifies rooted file and tree copying honors destination conflict policy.
#[cfg(unix)]
#[test]
fn test_rooted_copy_supports_file_and_tree_modes() {
    use std::os::unix::fs::{
        MetadataExt,
        PermissionsExt,
    };

    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    std::fs::write(directory.path().join("source.txt"), b"source")
        .expect("the source should be written");
    std::fs::set_permissions(
        directory.path().join("source.txt"),
        std::fs::Permissions::from_mode(0o640),
    )
    .expect("the source permissions should be set");
    std::fs::write(directory.path().join("destination.txt"), b"destination")
        .expect("the destination should be written");
    let file_system = open_rooted_file_system("rooted-copy", directory.path());
    let source = FsPath::parse("/source.txt").expect("the source should parse");
    let destination = FsPath::parse("/destination.txt")
        .expect("the destination should parse");

    let skipped = file_system
        .copy(
            &source,
            &destination,
            CopyOptions {
                conflict: CopyConflictPolicy::Skip,
                ..CopyOptions::file()
            },
        )
        .expect("the destination should be skipped");
    assert_eq!(1, skipped.stats.skipped);

    let copied = file_system
        .copy(
            &source,
            &destination,
            CopyOptions {
                conflict: CopyConflictPolicy::Overwrite,
                ..CopyOptions::file()
            },
        )
        .expect("the destination should be overwritten");
    assert_eq!(CopyMethod::Local, copied.method);
    assert_eq!(1, copied.stats.files);
    assert_eq!(6, copied.stats.bytes);
    assert_eq!(1, copied.stats.overwritten);
    assert_eq!(
        0o640,
        std::fs::metadata(directory.path().join("destination.txt"))
            .expect("the destination metadata should be readable")
            .mode()
            & 0o777,
    );

    std::fs::create_dir(directory.path().join("source-tree"))
        .expect("the source tree should be created");
    std::fs::set_permissions(
        directory.path().join("source-tree"),
        std::fs::Permissions::from_mode(0o750),
    )
    .expect("the tree permissions should be set");
    std::fs::write(directory.path().join("source-tree/value.txt"), b"tree")
        .expect("the tree fixture should be written");
    let tree = file_system
        .copy(
            &FsPath::parse("/source-tree").expect("the source should parse"),
            &FsPath::parse("/destination-tree")
                .expect("the destination should parse"),
            CopyOptions::tree(),
        )
        .expect("the tree should be copied");
    assert_eq!(1, tree.stats.files);
    assert_eq!(1, tree.stats.directories);
    assert_eq!(4, tree.stats.bytes);
    assert_eq!(
        0o750,
        std::fs::metadata(directory.path().join("destination-tree"))
            .expect("the destination tree metadata should be readable")
            .mode()
            & 0o777,
    );
}

/// Verifies rooted copy rejects self-targets and invalid modes before changing
/// the destination namespace.
#[cfg(unix)]
#[test]
fn test_rooted_copy_preflights_aliases_and_modes() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    std::fs::write(directory.path().join("value.txt"), b"preserved")
        .expect("the file should be written");
    std::fs::create_dir(directory.path().join("tree"))
        .expect("the tree should be created");
    let file_system =
        open_rooted_file_system("rooted-copy-preflight", directory.path());
    let file = FsPath::parse("/value.txt").expect("the path should parse");

    let self_error = file_system
        .copy(
            &file,
            &file,
            CopyOptions {
                conflict: CopyConflictPolicy::Overwrite,
                ..CopyOptions::file()
            },
        )
        .expect_err("self-copy should be rejected");
    assert_eq!(FsErrorKind::InvalidOptions, self_error.kind());
    assert_eq!(
        b"preserved",
        std::fs::read(directory.path().join("value.txt"))
            .expect("the original file should remain")
            .as_slice(),
    );

    let mode_error = file_system
        .copy(
            &FsPath::parse("/tree").expect("the source should parse"),
            &FsPath::parse("/missing/value.txt")
                .expect("the destination should parse"),
            CopyOptions {
                create_parent: true,
                ..CopyOptions::file()
            },
        )
        .expect_err("a mismatched mode should be rejected");
    assert_eq!(FsErrorKind::InvalidOptions, mode_error.kind());
    assert!(!directory.path().join("missing").exists());

    let descendant_error = file_system
        .copy(
            &FsPath::parse("/tree").expect("the source should parse"),
            &FsPath::parse("/tree/child")
                .expect("the destination should parse"),
            CopyOptions::tree(),
        )
        .expect_err("copying a tree into itself should be rejected");
    assert_eq!(FsErrorKind::InvalidOptions, descendant_error.kind());
    assert!(!directory.path().join("tree/child").exists());

    std::fs::write(directory.path().join("hard-source.txt"), b"hard-link")
        .expect("the hard-link source should be written");
    std::fs::hard_link(
        directory.path().join("hard-source.txt"),
        directory.path().join("hard-alias.txt"),
    )
    .expect("the hard-link alias should be created");
    let alias_error = file_system
        .copy(
            &FsPath::parse("/hard-source.txt")
                .expect("the source should parse"),
            &FsPath::parse("/hard-alias.txt")
                .expect("the destination should parse"),
            CopyOptions {
                conflict: CopyConflictPolicy::Overwrite,
                ..CopyOptions::file()
            },
        )
        .expect_err("a hard-link alias should be rejected");
    assert_eq!(FsErrorKind::InvalidOptions, alias_error.kind());
    assert_eq!(
        b"hard-link",
        std::fs::read(directory.path().join("hard-source.txt"))
            .expect("the hard-link source should remain")
            .as_slice(),
    );
}

/// Verifies default rooted whole-file writes publish atomically.
#[cfg(unix)]
#[test]
fn test_rooted_write_all_atomically_replaces_file() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    std::fs::write(directory.path().join("value.txt"), b"old")
        .expect("the original file should be created");
    let file_system =
        open_rooted_file_system("rooted-atomic", directory.path());
    let path = FsPath::parse("/value.txt").expect("the path should parse");

    let outcome = file_system
        .write_all(&path, b"replacement")
        .expect("the rooted replacement should succeed");

    assert_eq!(AchievedAtomicity::Atomic, outcome.atomicity);
    assert_eq!(PublicationMethod::AtomicRename, outcome.method);
    assert_eq!(
        b"replacement",
        std::fs::read(directory.path().join("value.txt"))
            .expect("the replacement should be readable")
            .as_slice(),
    );
}

/// Verifies rooted callers can explicitly request direct replacement.
#[cfg(unix)]
#[test]
fn test_rooted_open_writer_supports_direct_replacement() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system =
        open_rooted_file_system("rooted-direct", directory.path());
    let path = FsPath::parse("/value.txt").expect("the path should parse");
    let options = WriteOptions {
        atomicity: AtomicityRequirement::NotRequired,
        ..WriteOptions::default()
    };

    let mut writer = file_system
        .open_writer(&path, options)
        .expect("the direct writer should open");
    writer
        .write_fully(b"direct")
        .expect("the direct contents should be written");
    let outcome = writer.commit().expect("the direct write should commit");

    assert_eq!(AchievedAtomicity::NonAtomic, outcome.atomicity);
    assert_eq!(PublicationMethod::Direct, outcome.method);
}

/// Verifies a committed rooted direct writer releases its native descriptor.
#[cfg(target_os = "linux")]
#[test]
fn test_rooted_direct_commit_releases_native_descriptor() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system =
        open_rooted_file_system("rooted-direct-close", directory.path());
    let path = FsPath::parse("/value.txt").expect("the path should parse");
    let native_path = directory.path().join("value.txt");
    let options = WriteOptions {
        atomicity: AtomicityRequirement::NotRequired,
        ..WriteOptions::default()
    };
    let mut writer = file_system
        .open_writer(&path, options)
        .expect("the direct writer should open");
    writer
        .write_fully(b"direct")
        .expect("the direct contents should be written");
    assert_eq!(1, open_descriptor_count(&native_path));

    writer.commit().expect("the direct write should commit");

    assert_eq!(0, open_descriptor_count(&native_path));
}

/// Verifies required atomic create-new fails before creating parent entries.
#[cfg(unix)]
#[test]
fn test_rooted_open_writer_rejects_required_atomic_create_new() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system =
        open_rooted_file_system("rooted-create-new", directory.path());
    let path =
        FsPath::parse("/missing/value.txt").expect("the path should parse");
    let options = WriteOptions {
        create_parent: true,
        disposition: WriteDisposition::CreateNew,
        atomicity: AtomicityRequirement::Required,
        ..WriteOptions::default()
    };

    let error = file_system
        .open_writer(&path, options)
        .expect_err("atomic create-new should be rejected");

    assert_eq!(FsErrorKind::RequirementNotMet, error.kind());
    assert_eq!(FsOperation::OpenWriter, error.operation());
    assert!(!directory.path().join("missing").exists());
}

/// Verifies rooted stat reports the root, directories, and final symbolic
/// links.
#[cfg(unix)]
#[test]
fn test_stat_reports_root_directory_and_final_symbolic_link() {
    use std::os::unix::fs::symlink;

    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    std::fs::create_dir(directory.path().join("nested"))
        .expect("the nested directory should be created");
    std::fs::write(directory.path().join("value.txt"), b"value")
        .expect("the regular file should be created");
    symlink("value.txt", directory.path().join("value-link"))
        .expect("the final symbolic link should be created");
    let file_system = open_rooted_file_system("rooted-stat", directory.path());

    assert_eq!(
        FileKind::Directory,
        file_system
            .stat(&FsPath::root())
            .expect("the root should be statable")
            .kind,
    );
    let directory_path =
        FsPath::parse("/nested").expect("the path should parse");
    assert_eq!(
        FileKind::Directory,
        file_system
            .stat(&directory_path)
            .expect("the directory should be statable")
            .kind,
    );
    let link_path =
        FsPath::parse("/value-link").expect("the path should parse");
    assert_eq!(
        FileKind::Symlink,
        file_system
            .stat(&link_path)
            .expect("the final link should be statable")
            .kind,
    );

    let file_path = FsPath::parse("/value.txt").expect("the path should parse");
    let metadata = file_system
        .stat(&file_path)
        .expect("the file should be statable");
    assert!(metadata.accessed_at.is_some());
    assert!(metadata.modified_at.is_some());
}

/// Verifies rooted operations reject object-key literals that are not
/// canonical hierarchical paths.
#[cfg(unix)]
#[test]
fn test_rooted_stat_rejects_noncanonical_hierarchical_paths() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    std::fs::create_dir(directory.path().join("nested"))
        .expect("the nested directory should be created");
    std::fs::write(directory.path().join("value.txt"), b"value")
        .expect("the regular file should be created");
    let file_system =
        open_rooted_file_system("rooted-path-validation", directory.path());

    for literal in ["/./value.txt", "//value.txt", "/nested/../value.txt"] {
        let path =
            FsPath::parse_literal(literal).expect("the literal should parse");
        let error = file_system
            .stat(&path)
            .expect_err("the noncanonical path should be rejected");

        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::Stat, error.operation());
        assert_eq!(Some(&path), error.path());
        assert_eq!(Some("local-file"), error.provider());
    }
}

/// Verifies rooted paths decode canonical percent and non-UTF-8 components.
#[cfg(unix)]
#[test]
fn test_open_reader_decodes_canonical_native_components() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    std::fs::write(directory.path().join("100%ready.txt"), b"percent")
        .expect("the percent file should be created");
    let non_utf8_name = OsString::from_vec(vec![b'f', b'o', 0x80, b'o']);
    std::fs::write(directory.path().join(&non_utf8_name), b"non-utf8")
        .expect("the non-UTF-8 file should be created");
    let file_system = open_rooted_file_system("rooted-codec", directory.path());

    let percent_path =
        FsPath::parse("/100%25ready.txt").expect("the path should parse");
    assert_eq!(
        b"percent",
        file_system
            .read_all(&percent_path, 64)
            .expect("the percent file should be read")
            .as_slice(),
    );
    let non_utf8_path =
        FsPath::parse("/fo%80o").expect("the path should parse");
    assert_eq!(
        b"non-utf8",
        file_system
            .read_all(&non_utf8_path, 64)
            .expect("the non-UTF-8 file should be read")
            .as_slice(),
    );
}

/// Verifies unsupported write metadata is rejected before a rooted file opens.
#[cfg(unix)]
#[test]
fn test_open_writer_rejects_content_metadata() {
    let directory =
        tempfile::tempdir().expect("a temporary root should be created");
    let file_system =
        open_rooted_file_system("rooted-write-options", directory.path());
    let path = FsPath::parse("/value.txt").expect("the path should parse");
    let options = WriteOptions {
        content_type: Some("text/plain".to_owned()),
        ..WriteOptions::default()
    };

    let error = file_system
        .open_writer(&path, options)
        .expect_err("unsupported content metadata should be rejected");

    assert_eq!(FsErrorKind::InvalidOptions, error.kind());
    assert!(!directory.path().join("value.txt").exists());
}

/// Verifies opened locations distinguish different rooted authorities.
#[cfg(unix)]
#[test]
fn test_open_reader_distinguishes_rooted_filesystem_ids() {
    let first = tempfile::tempdir().expect("the first root should be created");
    let second =
        tempfile::tempdir().expect("the second root should be created");
    std::fs::write(first.path().join("value.txt"), b"first")
        .expect("the first file should be created");
    std::fs::write(second.path().join("value.txt"), b"second")
        .expect("the second file should be created");
    let first_file_system =
        open_rooted_file_system("rooted-first", first.path());
    let second_file_system =
        open_rooted_file_system("rooted-second", second.path());
    let path = FsPath::parse("/value.txt").expect("the path should parse");

    let first_reader = first_file_system
        .open_reader(&path, Default::default())
        .expect("the first file should open");
    let second_reader = second_file_system
        .open_reader(&path, Default::default())
        .expect("the second file should open");

    assert_ne!(
        first_reader.info().location(),
        second_reader.info().location(),
    );
}
