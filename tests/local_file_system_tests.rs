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
    FileSystemProperties,
    FsErrorKind,
    FsOperation,
    FsPath,
    NativePathCodec,
    OsStrPathCodec,
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

/// Confirms the host implementation advertises its metadata operation.
#[test]
fn test_host_advertises_stat_capability() {
    let fs = LocalFileSystem::host();

    assert_eq!(
        fs.capabilities(),
        FileSystemCapabilities::default().with(FileSystemCapability::Stat),
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
