// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider ceilings and caller behavior remain independent during migration.

use std::time::Duration;

use qubit_fs::copy::CopyOptions;
use qubit_fs::directory::ListFilter;
use qubit_fs::directory::ListOptions;
use qubit_fs::directory::ListScope;
use qubit_fs::error::FsErrorKind;
use qubit_fs::path::Path;
use qubit_fs_local::LocalCopyResourceLimits;
use qubit_fs_local::LocalDeleteResourceLimits;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalListResourceLimits;
use qubit_fs_local::LocalResourcePolicy;

/// Constructs finite, non-time-sensitive test ceilings for all operations.
fn policy(list_entries: usize, copy_bytes: u64) -> LocalResourcePolicy {
    let deadline = Duration::from_secs(30);
    LocalResourcePolicy::bounded(
        LocalListResourceLimits::new(8, list_entries, 4096, 8, deadline).expect("list ceilings"),
        LocalCopyResourceLimits::new(8, 100, copy_bytes, 8, deadline).expect("copy ceilings"),
        LocalDeleteResourceLimits::new(8, 100, 4096, deadline),
    )
}

/// Copy enforces the smaller request/provider byte ceiling, not only one side.
#[test]
fn test_copy_request_and_provider_limits_intersect() {
    for (request_bytes, payload_size, succeeds) in [(100, 11, false), (5, 6, false), (5, 4, true)] {
        let root = tempfile::tempdir().expect("isolated fixture");
        std::fs::write(root.path().join("source"), vec![b'x'; payload_size]).expect("source payload");
        let filesystem = LocalFileSystems::rooted(root.path(), policy(100, 10)).expect("bounded filesystem");
        let result = filesystem.copy(
            &Path::parse("/source").expect("source path"),
            &Path::parse("/target").expect("target path"),
            CopyOptions::file().with_max_bytes(Some(request_bytes)),
        );
        if succeeds {
            let _ = result.expect("payload within both ceilings");
            assert_eq!(
                std::fs::read(root.path().join("target")).expect("copied payload").len(),
                payload_size
            );
        } else {
            assert_eq!(
                result.expect_err("tighter ceiling must reject").error().kind(),
                FsErrorKind::ResourceLimitExceeded
            );
            assert!(!root.path().join("target").exists());
        }
    }
}

/// Caller limits count matched entries while provider limits still count native
/// work.
#[test]
fn test_filtered_listing_keeps_distinct_counting_scopes() {
    let root = tempfile::tempdir().expect("isolated fixture");
    std::fs::create_dir(root.path().join("matched")).expect("intermediate directory");
    std::fs::write(root.path().join("matched/leaf"), b"payload").expect("matched leaf");
    let options = ListOptions::default()
        .with_max_entries(Some(1))
        .with_filter(Some(ListFilter::Subtree("matched/leaf".to_owned())));
    let filesystem =
        LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("unbounded provider");
    let mut entries = filesystem
        .list(
            &ListScope::Path((Path::parse("/").expect("root path")).clone()),
            options.clone(),
        )
        .expect("filtered listing");
    assert!(
        entries
            .next_entry()
            .expect("unmatched ancestor must not exhaust request limit")
            .is_some()
    );
    let bounded = LocalFileSystems::rooted(root.path(), policy(1, 100)).expect("bounded provider");
    let mut entries = bounded
        .list(
            &ListScope::Path((Path::parse("/").expect("root path")).clone()),
            options,
        )
        .expect("bounded filtered listing");
    assert_eq!(
        entries
            .next_entry()
            .expect_err("provider work ceiling remains active before filtering")
            .kind(),
        FsErrorKind::ResourceLimitExceeded
    );
}

/// Logical dot normalization still occurs before provider-native traversal.
#[cfg(unix)]
#[test]
fn test_logical_parent_components_keep_portable_path_semantics() {
    use std::fs;
    use std::os::unix::fs::symlink;

    use qubit_fs::read::ReadOptions;

    let root = tempfile::tempdir().expect("isolated logical-path fixture");
    fs::create_dir(root.path().join("a")).expect("logical parent");
    fs::create_dir_all(root.path().join("b/inner")).expect("native link destination");
    fs::write(root.path().join("a/config"), b"A").expect("logical target");
    fs::write(root.path().join("b/config"), b"B").expect("native target");
    symlink("../b/inner", root.path().join("a/link")).expect("link fixture");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted facade");
    let path = Path::parse("/a/link/../config").expect("logical path");
    assert_eq!(path.as_str(), "/a/config");
    assert_eq!(
        filesystem
            .read_prefix(&path, ReadOptions::default(), 8)
            .expect("portable read"),
        b"A"
    );
    assert_eq!(
        fs::read(root.path().join("a/link/../config")).expect("native read"),
        b"B"
    );
}
