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
    NativePathCodec,
    OsStrPathCodec,
    ReadOptions,
};
use qubit_fs_local::LocalFileSystem;
use qubit_spi::ProviderId;

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
    let text = OsStrPathCodec
        .decode(path.as_os_str())
        .expect("decode native test path");
    FsPath::parse(text.as_ref()).expect("parse canonical test path")
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
    assert_eq!(
        error.provider(),
        Some(&ProviderId::new("local-file").expect("parse provider id")),
    );
}

/// Confirms the host implementation advertises only optional read capability.
#[test]
fn test_host_advertises_read_capability() {
    let fs = LocalFileSystem::host();

    assert_eq!(
        fs.capabilities(),
        FileSystemCapabilities::default().with(FileSystemCapability::Read),
    );
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
    assert_eq!(
        error.provider(),
        Some(&ProviderId::new("local-file").expect("parse provider id")),
    );
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
    assert_eq!(
        error.provider(),
        Some(&ProviderId::new("local-file").expect("parse provider id")),
    );
}
