// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::error::Error;

use qubit_fs::copy::CopyFailureState;
use qubit_fs::copy::CopyOptions;
use qubit_fs::directory::CreateDirectoryOptions;
use qubit_fs::error::FsEffectState;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::path::Path;
use qubit_fs::write::WriteDisposition;
use qubit_fs::write::WriteFailureState;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;
use qubit_io::Output;
use qubit_local_files::LocalFileError;
use qubit_local_files::test_support::install_test_fault;

#[test]
fn publication_incomplete_retains_partial_effect_and_native_source() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let target = host_path_to_logical(&root.path().join("first/second"))
        .expect("target should be representable");
    let filesystem = LocalFileSystems::host(LocalResourcePolicy::unbounded())
        .expect("host filesystem should construct");
    let _fault = install_test_fault("host-create-directory-component-second")
        .expect("fault controller should install");

    let error = filesystem
        .create_directory(
            &target,
            CreateDirectoryOptions::default().with_recursive(true),
        )
        .expect_err("second component fault should interrupt publication");

    assert_eq!(FsErrorKind::Io, error.kind());
    assert_eq!(Some(FsEffectState::PartiallyApplied), error.effect_state());
    assert_eq!(FsOperation::CreateDir, error.operation());
    assert_eq!(Some(&target), error.path());
    assert_eq!(Some("local-file"), error.provider());
    assert!(
        error
            .source()
            .and_then(|source| source.downcast_ref::<LocalFileError>())
            .is_some()
    );
}

#[test]
fn typed_copy_failure_exposes_unchanged_effect_and_request_context() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let source = Path::parse("/missing").expect("valid source path");
    let target = Path::parse("/target").expect("valid target path");
    let filesystem =
        LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem should construct");

    let failure = filesystem
        .copy(&source, &target, CopyOptions::file())
        .expect_err("missing source should fail");

    assert_eq!(CopyFailureState::Unchanged, failure.state());
    assert_eq!(
        Some(FsEffectState::Unchanged),
        failure.error().effect_state()
    );
    assert_eq!(Some(&source), failure.error().path());
    assert_eq!(Some(&target), failure.error().target());
    assert!(failure.error().source().is_some());
}

#[test]
fn writer_conflict_retains_unchanged_effect_and_native_source() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let target = path("/target");
    let filesystem =
        LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem should construct");
    let mut writer = filesystem
        .open_writer(
            &target,
            WriteOptions::default()
                .with_disposition(WriteDisposition::CreateNew),
        )
        .expect("writer should open before the conflict");
    Output::write_fully(&mut writer, b"staged")
        .expect("writer should accept bytes");
    std::fs::write(root.path().join("target"), b"concurrent")
        .expect("concurrent target should be installed");

    let failure = writer
        .commit()
        .expect_err("concurrent target should conflict");
    assert_eq!(WriteFailureState::NotPublished, failure.state());
    assert_eq!(
        Some(FsEffectState::Unchanged),
        failure.error().effect_state()
    );
    assert!(
        failure
            .error()
            .source()
            .and_then(|source| source.downcast_ref::<LocalFileError>())
            .is_some()
    );
}

fn path(value: &str) -> Path {
    Path::parse(value).expect("test path should be valid")
}
