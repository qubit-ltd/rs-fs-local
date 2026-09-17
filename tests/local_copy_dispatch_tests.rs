// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::copy::CopyFailureState;
use qubit_fs::copy::CopyMethod;
use qubit_fs::copy::CopyOptions;
use qubit_fs::copy::CopyStats;
use qubit_fs::copy::MetadataPreservePolicy;
use qubit_fs::error::FsErrorKind;
use qubit_fs::path::Path;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;

#[test]
fn test_local_unrepresentable_copy_fails_before_target_creation() {
    let root = tempfile::tempdir().expect("root should exist");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");
    let source = Path::parse("/missing-source").expect("source path should parse");
    let target = Path::parse("/new-parent/target").expect("target path should parse");
    let failure = filesystem
        .copy(
            &source,
            &target,
            CopyOptions::file()
                .with_create_parent(true)
                .with_preserve_metadata(MetadataPreservePolicy::UserMetadata),
        )
        .expect_err("unrepresentable option must be terminal");

    assert_eq!(FsErrorKind::RequirementNotMet, failure.error().kind());
    assert_eq!(CopyFailureState::Unchanged, failure.state());
    assert_eq!(&CopyStats::default(), failure.partial_stats());
    assert_eq!(Some(&source), failure.error().path());
    assert_eq!(Some(&target), failure.error().target());
    assert!(!root.path().join("new-parent").exists());
}

#[test]
fn test_local_expressible_copy_is_native_without_facade_fallback() {
    let root = tempfile::tempdir().expect("root should exist");
    std::fs::write(root.path().join("source"), b"payload").expect("source fixture should be written");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");
    let source = Path::parse("/source").expect("source path should parse");
    let target = Path::parse("/target").expect("target path should parse");
    let outcome = filesystem
        .copy(&source, &target, CopyOptions::file())
        .expect("native copy should succeed");

    assert_eq!(CopyMethod::Native, outcome.method());
    assert!(!outcome.used_fallback());
    assert_eq!(
        b"payload",
        std::fs::read(root.path().join("target"))
            .expect("target should be written")
            .as_slice()
    );
}
