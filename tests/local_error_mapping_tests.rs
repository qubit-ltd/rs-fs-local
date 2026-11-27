// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::time::Duration;

use qubit_fs::copy::CopyFailureState;
use qubit_fs::copy::CopyOptions;
use qubit_fs::copy::CopyStats;
use qubit_fs::directory::CreateDirectoryOptions;
use qubit_fs::directory::DeleteOptions;
use qubit_fs::directory::ListOptions;
use qubit_fs::directory::ListScope;
use qubit_fs::error::FsEffectState;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::path::Path;
use qubit_fs::rename::RenameFailureState;
use qubit_fs::rename::RenameOptions;
#[cfg(windows)]
use qubit_fs::write::WriteAbortOutcome;
use qubit_fs::write::WriteDisposition;
use qubit_fs::write::WriteFailureState;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;
use qubit_io::Output;
use qubit_local_files::LocalFileError;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::outcome::LocalCopyFailure;
use qubit_local_files::test_support::install_test_fault;

#[test]
fn test_publication_incomplete_retains_partial_effect_and_native_source() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let target = host_path_to_logical(&root.path().join("first/second")).expect("target should be representable");
    let filesystem =
        LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host filesystem should construct");
    let _fault = install_test_fault("host-create-directory-component-second").expect("fault controller should install");

    let error = filesystem
        .create_directory(&target, CreateDirectoryOptions::default().with_recursive(true))
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
fn test_rooted_recursive_create_failure_projects_partial_child_path() {
    let root = tempfile::tempdir().expect("rooted fixture root should exist");
    let path = Path::parse("created/blocked").expect("relative rooted path should parse");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");
    let _fault =
        install_test_fault("rooted-create-directory-component-second").expect("rooted create fault should install");

    let error = filesystem
        .create_directory(&path, CreateDirectoryOptions::default().with_recursive(true))
        .expect_err("injected second-component error should interrupt creation");

    assert_eq!(FsErrorKind::Io, error.kind());
    assert_eq!(FsOperation::CreateDir, error.operation());
    assert_eq!(Some(FsEffectState::PartiallyApplied), error.effect_state());
    assert_eq!(Some(&path), error.path());
    assert_eq!(Some(&path), error.failure_path());
    assert_eq!(Some("local-file"), error.provider());
    assert!(
        error
            .source()
            .and_then(|source| source.downcast_ref::<LocalFileError>())
            .is_some()
    );
    assert!(root.path().join("created").is_dir());
    assert!(!root.path().join("created/blocked").exists());
}

#[test]
fn test_host_and_rooted_lazy_listing_failures_project_native_coordinates() {
    for rooted in [false, true] {
        let root = tempfile::tempdir().expect("listing fixture root should exist");
        std::fs::create_dir(root.path().join("nested")).expect("listing directory should exist");
        std::fs::write(root.path().join("nested/entry"), b"entry").expect("listing entry should exist");
        let filesystem = if rooted {
            LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("Rooted filesystem")
        } else {
            LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("Host filesystem")
        };
        let requested = if rooted {
            Path::parse("nested").expect("relative Rooted path")
        } else {
            host_path_to_logical(&root.path().join("nested")).expect("absolute Host path")
        };
        let mut stream = filesystem
            .list(&ListScope::Path(requested.clone()), ListOptions::default())
            .expect("lazy list should open before traversal");
        let _fault = install_test_fault("walker-entry").expect("walker fault should install");

        let error = stream
            .next_entry()
            .expect_err("injected lazy walker error should surface");

        assert_eq!(FsOperation::List, error.operation(), "rooted={rooted}");
        assert_eq!(Some("local-file"), error.provider(), "rooted={rooted}");
        assert_eq!(
            Some(&requested),
            error.path(),
            "request path should remain available; rooted={rooted}"
        );
        let failure_path = error.failure_path().expect("known walker namespace should project");
        assert_eq!(rooted, !failure_path.is_absolute(), "rooted={rooted}");
        assert!(
            failure_path.as_str().ends_with("nested"),
            "failure path: {failure_path}"
        );
        assert!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<LocalFileError>())
                .is_some()
        );
    }
}

#[test]
fn test_host_and_rooted_injected_file_copy_failures_retain_effect_facts() {
    for (rooted, fault) in [(false, "copy-staging-copy"), (true, "rooted-copy-file-contents-native")] {
        let root = tempfile::tempdir().expect("copy fixture root should exist");
        let native_source = root.path().join("source");
        let native_parent = root.path().join("created-parent");
        let native_target = native_parent.join("target");
        std::fs::write(&native_source, b"payload").expect("copy source should exist");
        let filesystem = if rooted {
            LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("Rooted filesystem")
        } else {
            LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("Host filesystem")
        };
        let source = if rooted {
            Path::parse("source").expect("relative Rooted source")
        } else {
            host_path_to_logical(&native_source).expect("absolute Host source")
        };
        let target = if rooted {
            Path::parse("created-parent/target").expect("relative Rooted target")
        } else {
            host_path_to_logical(&native_target).expect("absolute Host target")
        };
        let _fault = install_test_fault(fault).expect("copy fault should install");

        let failure = filesystem
            .copy(&source, &target, CopyOptions::file().with_create_parent(true))
            .expect_err("injected file contents fault should fail the copy");

        let expected_state = if rooted {
            CopyFailureState::Indeterminate
        } else {
            CopyFailureState::PartiallyPublished
        };
        assert_eq!(expected_state, failure.state(), "rooted={rooted}");
        let expected_effect = if rooted {
            FsEffectState::Indeterminate
        } else {
            FsEffectState::PartiallyApplied
        };
        assert_eq!(Some(expected_effect), failure.error().effect_state(), "rooted={rooted}");
        assert_eq!(Some(&source), failure.error().path(), "rooted={rooted}");
        assert_eq!(Some(&target), failure.error().target(), "rooted={rooted}");
        assert_eq!(Some(&source), failure.error().failure_path(), "rooted={rooted}");
        assert_eq!(Some(&target), failure.error().failure_target(), "rooted={rooted}");
        assert_eq!(&CopyStats::default(), failure.partial_stats(), "rooted={rooted}");
        assert_eq!(Some("local-file"), failure.error().provider(), "rooted={rooted}");
        let native_failure = failure
            .error()
            .source()
            .and_then(|source| source.downcast_ref::<LocalCopyFailure>())
            .expect("native typed copy failure should remain the cause");
        let native_cause = native_failure
            .source()
            .and_then(|source| source.downcast_ref::<LocalFileError>())
            .expect("native copy failure should retain its LocalFileError cause");
        assert_eq!(LocalFileErrorKind::Io, native_cause.kind(), "rooted={rooted}");
        assert_eq!(LocalFileOperation::Copy, native_cause.operation(), "rooted={rooted}");
        assert!(
            native_parent.is_dir(),
            "created parent is the partial effect; rooted={rooted}"
        );
        assert!(!native_target.exists(), "target remains unpublished; rooted={rooted}");
    }
}

#[cfg(unix)]
#[test]
fn test_host_and_rooted_injected_symlink_copy_failures_retain_unchanged_facts() {
    for (rooted, fault) in [
        (false, "local-fs-copy-symlink-create"),
        (true, "rooted-copy-symlink-create"),
    ] {
        let root = tempfile::tempdir().expect("copy fixture root should exist");
        let native_source = root.path().join("source-link");
        let native_target = root.path().join("target-link");
        std::os::unix::fs::symlink("referent", &native_source).expect("source symlink should exist");
        let filesystem = if rooted {
            LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("Rooted filesystem")
        } else {
            LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("Host filesystem")
        };
        let source = if rooted {
            Path::parse("source-link").expect("relative Rooted source")
        } else {
            host_path_to_logical(&native_source).expect("absolute Host source")
        };
        let target = if rooted {
            Path::parse("target-link").expect("relative Rooted target")
        } else {
            host_path_to_logical(&native_target).expect("absolute Host target")
        };
        let _fault = install_test_fault(fault).expect("symlink copy fault should install");

        let failure = filesystem
            .copy(&source, &target, CopyOptions::file())
            .expect_err("injected link creation failure should stop before publication");

        assert_eq!(CopyFailureState::Unchanged, failure.state(), "rooted={rooted}");
        assert_eq!(
            Some(FsEffectState::Unchanged),
            failure.error().effect_state(),
            "rooted={rooted}"
        );
        assert_eq!(Some(&source), failure.error().path(), "rooted={rooted}");
        assert_eq!(Some(&target), failure.error().target(), "rooted={rooted}");
        assert_eq!(Some(&source), failure.error().failure_path(), "rooted={rooted}");
        assert_eq!(Some(&target), failure.error().failure_target(), "rooted={rooted}");
        assert_eq!(&CopyStats::default(), failure.partial_stats(), "rooted={rooted}");
        assert_eq!(Some("local-file"), failure.error().provider(), "rooted={rooted}");
        let native_failure = failure
            .error()
            .source()
            .and_then(|source| source.downcast_ref::<LocalCopyFailure>())
            .expect("native typed copy failure should remain the cause");
        let native_cause = native_failure
            .source()
            .and_then(|source| source.downcast_ref::<LocalFileError>())
            .expect("native copy failure should retain its LocalFileError cause");
        assert_eq!(LocalFileErrorKind::Io, native_cause.kind(), "rooted={rooted}");
        assert_eq!(LocalFileOperation::Copy, native_cause.operation(), "rooted={rooted}");
        assert!(
            std::fs::symlink_metadata(&native_source)
                .expect("source remains")
                .file_type()
                .is_symlink()
        );
        assert!(std::fs::symlink_metadata(&native_target).is_err(), "rooted={rooted}");
    }
}

#[test]
fn test_typed_copy_failure_exposes_unchanged_effect_and_request_context() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let source = Path::parse("/missing").expect("valid source path");
    let target = Path::parse("/target").expect("valid target path");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");

    let failure = filesystem
        .copy(&source, &target, CopyOptions::file())
        .expect_err("missing source should fail");

    assert_eq!(CopyFailureState::Unchanged, failure.state());
    assert_eq!(Some(FsEffectState::Unchanged), failure.error().effect_state());
    assert_eq!(Some(&source), failure.error().path());
    assert_eq!(Some(&target), failure.error().target());
    assert!(failure.error().source().is_some());
}

#[test]
fn test_typed_rename_failure_exposes_unchanged_effect_and_request_context() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let source = Path::parse("/missing").expect("valid source path");
    let target = Path::parse("/target").expect("valid target path");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");

    let failure = filesystem
        .rename(&source, &target, RenameOptions::default())
        .expect_err("missing source should fail");

    assert_eq!(RenameFailureState::Unchanged, failure.state());
    assert_eq!(Some(FsEffectState::Unchanged), failure.error().effect_state());
    assert_eq!(Some(&source), failure.error().path());
    assert_eq!(Some(&target), failure.error().target());
    assert_eq!(Some("local-file"), failure.error().provider());
    assert!(failure.error().source().is_some());
}

#[test]
fn test_copy_and_rename_path_conversion_failures_retain_both_operands() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    std::fs::write(root.path().join("source"), b"payload").expect("source should exist");
    let target = Path::parse("/invalid%00target").expect("NUL target remains a logical path");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");

    let copy = filesystem
        .copy(
            &Path::parse("/source").expect("valid source"),
            &target,
            CopyOptions::file(),
        )
        .expect_err("invalid target conversion should fail");
    assert_eq!(CopyFailureState::Unchanged, copy.state());
    assert_eq!(FsOperation::Copy, copy.error().operation());
    assert_eq!(FsErrorKind::InvalidPath, copy.error().kind());
    assert_eq!(Some(&Path::parse("/source").unwrap()), copy.error().path());
    assert_eq!(Some(&target), copy.error().target());
    assert_eq!(Some(&target), copy.error().failure_target());
    assert_eq!(Some("local-file"), copy.error().provider());
    assert_eq!(Some(FsEffectState::Unchanged), copy.error().effect_state());
    assert!(
        copy.error()
            .source()
            .and_then(|cause| cause.downcast_ref::<LocalFileError>())
            .is_some()
    );

    let rename = filesystem
        .rename(
            &Path::parse("/source").expect("valid source"),
            &target,
            RenameOptions::default(),
        )
        .expect_err("invalid target conversion should fail");
    assert_eq!(RenameFailureState::Unchanged, rename.state());
    assert_eq!(FsOperation::Rename, rename.error().operation());
    assert_eq!(FsErrorKind::InvalidPath, rename.error().kind());
    assert_eq!(Some(&Path::parse("/source").unwrap()), rename.error().path());
    assert_eq!(Some(&target), rename.error().target());
    assert_eq!(Some(&target), rename.error().failure_target());
    assert_eq!(Some("local-file"), rename.error().provider());
    assert_eq!(Some(FsEffectState::Unchanged), rename.error().effect_state());
    assert!(
        rename
            .error()
            .source()
            .and_then(|cause| cause.downcast_ref::<LocalFileError>())
            .is_some()
    );

    let invalid_source = Path::parse("/invalid%00source").expect("NUL source remains a logical path");
    let valid_target = Path::parse("/valid-target").expect("valid target");
    let copy = filesystem
        .copy(&invalid_source, &valid_target, CopyOptions::file())
        .expect_err("invalid source conversion should fail");
    assert_eq!(CopyFailureState::Unchanged, copy.state());
    assert_eq!(Some(&invalid_source), copy.error().path());
    assert_eq!(Some(&valid_target), copy.error().target());
    assert_eq!(Some(&invalid_source), copy.error().failure_path());
    assert_eq!(FsOperation::Copy, copy.error().operation());
    assert!(
        copy.error()
            .source()
            .and_then(|cause| cause.downcast_ref::<LocalFileError>())
            .is_some()
    );

    let rename = filesystem
        .rename(&invalid_source, &valid_target, RenameOptions::default())
        .expect_err("invalid source conversion should fail");
    assert_eq!(RenameFailureState::Unchanged, rename.state());
    assert_eq!(Some(&invalid_source), rename.error().path());
    assert_eq!(Some(&valid_target), rename.error().target());
    assert_eq!(Some(&invalid_source), rename.error().failure_path());
    assert_eq!(FsOperation::Rename, rename.error().operation());
    assert!(
        rename
            .error()
            .source()
            .and_then(|cause| cause.downcast_ref::<LocalFileError>())
            .is_some()
    );
}

#[test]
fn test_rooted_copy_failure_paths_follow_relative_and_absolute_request_forms() {
    for source_absolute in [false, true] {
        for target_absolute in [false, true] {
            let root = tempfile::tempdir().expect("rooted fixture should exist");
            let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
                .expect("rooted filesystem should construct");
            let source =
                Path::parse(if source_absolute { "/missing" } else { "missing" }).expect("source path should parse");
            let target =
                Path::parse(if target_absolute { "/target" } else { "target" }).expect("target path should parse");

            let failure = filesystem
                .copy(&source, &target, CopyOptions::file().with_deadline(Some(Duration::MAX)))
                .expect_err("missing source should fail before publication");

            assert_eq!(FsErrorKind::InvalidOptions, failure.error().kind());
            assert_eq!(CopyFailureState::Unchanged, failure.state());
            assert_eq!(Some(FsEffectState::Unchanged), failure.error().effect_state());
            assert_eq!(&CopyStats::default(), failure.partial_stats());
            let failure_path = failure
                .error()
                .failure_path()
                .expect("native source coordinate should project");
            assert_eq!(source_absolute, failure_path.is_absolute());
            assert_eq!(source.as_str(), failure_path.as_str());
            assert_eq!(Some(&source), failure.error().path());
            assert_eq!(Some(&target), failure.error().target());
            assert!(!root.path().join("target").exists());
            assert!(!root.path().join("missing").exists());
            let native_failure = failure
                .error()
                .source()
                .and_then(|source| source.downcast_ref::<LocalCopyFailure>())
                .expect("native copy failure should remain the direct cause");
            assert_eq!(LocalFileErrorKind::InvalidOptions, native_failure.error().kind());
        }
    }

    let root = tempfile::tempdir().expect("Host fixture should exist");
    let filesystem = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("Host should construct");
    let source = host_path_to_logical(&root.path().join("missing")).expect("absolute Host source should convert");
    let target = host_path_to_logical(&root.path().join("target")).expect("absolute Host target should convert");
    let failure = filesystem
        .copy(&source, &target, CopyOptions::file().with_deadline(Some(Duration::MAX)))
        .expect_err("maximum elapsed deadline must be rejected before provider effects");
    assert_eq!(FsErrorKind::InvalidOptions, failure.error().kind());
    assert_eq!(CopyFailureState::Unchanged, failure.state());
    assert_eq!(Some(FsEffectState::Unchanged), failure.error().effect_state());
    assert_eq!(&CopyStats::default(), failure.partial_stats());
    assert_eq!(Some(&source), failure.error().path());
    assert_eq!(Some(&target), failure.error().target());
    assert!(failure.error().source().is_some());
    assert!(!root.path().join("target").exists());
}

#[test]
fn test_writer_conflict_retains_unchanged_effect_and_native_source() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let target = path("/target");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");
    let mut writer = filesystem
        .open_writer(
            &target,
            WriteOptions::default().with_disposition(WriteDisposition::CreateNew),
        )
        .expect("writer should open before the conflict");
    Output::write_fully(&mut writer, b"staged").expect("writer should accept bytes");
    std::fs::write(root.path().join("target"), b"concurrent").expect("concurrent target should be installed");

    let failure = writer.commit().expect_err("concurrent target should conflict");
    #[cfg(not(windows))]
    assert_eq!(WriteFailureState::NotPublished, failure.state());
    #[cfg(windows)]
    assert_eq!(WriteFailureState::RetryableNotPublished, failure.state());
    assert_eq!(Some(FsEffectState::Unchanged), failure.error().effect_state());
    assert!(
        failure
            .error()
            .source()
            .and_then(|source| source.downcast_ref::<LocalFileError>())
            .is_some()
    );
    assert_eq!(
        std::fs::read(root.path().join("target")).expect("concurrent target survives"),
        b"concurrent"
    );
    #[cfg(windows)]
    assert_eq!(
        writer.abort().expect("retained native writer can abort"),
        WriteAbortOutcome::NotPublished
    );
}

#[test]
fn test_partial_delete_mapping_retains_native_failure_path_and_effect() {
    for (rooted, fault) in [
        (false, "host-delete-directory-entry-second"),
        (true, "rooted-delete-directory-entry-second"),
    ] {
        let root = tempfile::tempdir().expect("fixture root should exist");
        let native_target = root.path().join("tree");
        std::fs::create_dir(&native_target).expect("tree should exist");
        std::fs::write(native_target.join("first"), b"first").expect("first entry should exist");
        std::fs::write(native_target.join("second"), b"second").expect("second entry should exist");
        let target = if rooted {
            Path::parse("tree").expect("relative rooted target should parse")
        } else {
            host_path_to_logical(&native_target).expect("target should be representable")
        };
        let filesystem = if rooted {
            LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
                .expect("rooted filesystem should construct")
        } else {
            LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host filesystem should construct")
        };
        let _fault = install_test_fault(fault).expect("fault controller should install");

        let error = filesystem
            .delete_directory(&target, DeleteOptions::default().with_recursive(true))
            .expect_err("second entry fault should interrupt deletion");

        assert_eq!(FsErrorKind::Io, error.kind());
        assert_eq!(Some(FsEffectState::PartiallyApplied), error.effect_state());
        assert_eq!(Some(&target), error.path());
        let native = error
            .source()
            .and_then(|source| source.downcast_ref::<LocalFileError>())
            .expect("mapped error should retain the native source");
        assert_eq!(LocalFileErrorKind::PublicationIncomplete, native.kind());
        let native_path = native.path().expect("native failure path should be retained");
        let failure_path = error
            .failure_path()
            .expect("portable failure coordinate should be projected");
        assert!(
            failure_path.as_str().starts_with(target.as_str()),
            "projected failure coordinate {failure_path} should remain under request {target}"
        );
        assert_eq!(!rooted, failure_path.is_absolute());
        if rooted {
            assert!(
                native_path.starts_with("/tree"),
                "native rooted coordinate: {native_path:?}"
            );
        } else {
            assert!(
                native_path.starts_with(&native_target),
                "native Host coordinate: {native_path:?}"
            );
        }
        assert_eq!(1, std::fs::read_dir(&native_target).expect("tree remains").count());
    }
}

#[test]
fn test_non_recursive_delete_maps_nonempty_directory_to_conflict() {
    for rooted in [false, true] {
        let root = tempfile::tempdir().expect("fixture root should exist");
        let directory = root.path().join("non-empty");
        std::fs::create_dir(&directory).expect("directory should exist");
        std::fs::write(directory.join("sentinel"), b"keep").expect("entry should exist");
        let filesystem = if rooted {
            LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
                .expect("rooted filesystem should construct")
        } else {
            LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host filesystem should construct")
        };
        let path = if rooted {
            Path::parse("/non-empty").expect("rooted path should parse")
        } else {
            host_path_to_logical(&directory).expect("host path should convert")
        };

        let error = filesystem
            .delete_directory(&path, DeleteOptions::default())
            .expect_err("non-recursive deletion must preserve a nonempty directory");

        assert_eq!(FsErrorKind::Conflict, error.kind());
        assert_eq!(Some(&path), error.path());
        assert!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<LocalFileError>())
                .is_some()
        );
        assert_eq!(
            b"keep".as_slice(),
            std::fs::read(directory.join("sentinel")).expect("sentinel remains")
        );
    }
}

fn path(value: &str) -> Path {
    Path::parse(value).expect("test path should be valid")
}
