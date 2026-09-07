// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public behavior coverage for retained directory-listing options.

use qubit_fs::directory::CreateDirectoryOptions;
use qubit_fs::directory::ListOptions;
use qubit_fs::error::FsErrorKind;
use qubit_fs::path::Path;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalCopyResourceLimits;
use qubit_fs_local::LocalDeleteResourceLimits;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalListResourceLimits;
use qubit_fs_local::LocalResourcePolicy;

/// Prefix filtering includes the exact subtree and preserves requested
/// metadata without exposing sibling entries.
#[test]
fn test_listing_options_filter_prefix_and_include_metadata() {
    let root = tempfile::tempdir().expect("listing root must be created");
    let file_system =
        LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem must open");
    let list_root =
        Path::parse("/reports").expect("listing path must be valid");
    let matching_directory =
        Path::parse("/reports/nested").expect("matching path must be valid");
    let matching_file = Path::parse("/reports/nested/report.txt")
        .expect("matching file path must be valid");
    let sibling_directory = Path::parse("/reports/nested-other")
        .expect("sibling path must be valid");
    let sibling_file = Path::parse("/reports/nested-other/report.txt")
        .expect("sibling file path must be valid");

    for directory in [&list_root, &matching_directory, &sibling_directory] {
        file_system
            .create_directory(directory, CreateDirectoryOptions::default())
            .expect("fixture directory must be created");
    }
    for file in [&matching_file, &sibling_file] {
        file_system
            .write_all(file, b"payload", WriteOptions::default())
            .expect("fixture file must be written");
    }

    let mut stream = file_system
        .list(
            &list_root,
            ListOptions::default()
                .with_prefix(Some("nested".to_owned()))
                .with_include_metadata(true),
        )
        .expect("prefix-filtered listing must open");
    let mut entries = Vec::new();
    while let Some(entry) = stream
        .next_entry()
        .expect("prefix-filtered listing must not fail")
    {
        assert!(
            entry.metadata.is_some(),
            "requested metadata must be present for {}",
            entry.path
        );
        entries.push(entry.path.as_str().to_owned());
    }
    entries.sort();

    assert_eq!(
        vec![
            "/reports/nested".to_owned(),
            "/reports/nested/report.txt".to_owned(),
        ],
        entries
    );
}

#[test]
fn prefix_request_budget_counts_only_matching_entries() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("nested")).expect("parent");
    std::fs::write(root.path().join("nested/item"), b"x").expect("child");
    let fs =
        LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted");
    let mut stream = fs
        .list(
            &Path::root(),
            ListOptions::default()
                .with_prefix(Some("nested/item".to_owned()))
                .with_max_entries(Some(1)),
        )
        .expect("list");
    assert_eq!(
        Path::parse("/nested/item").expect("path"),
        stream
            .next_entry()
            .expect("unmatched parent does not consume returned budget")
            .expect("matching child")
            .path
    );
    assert!(stream.next_entry().expect("complete").is_none());
}

#[test]
fn prefix_zero_return_budget_allows_no_matches() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("unmatched"), b"x").expect("fixture");
    let fs =
        LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted");
    let mut stream = fs
        .list(
            &Path::root(),
            ListOptions::default()
                .with_prefix(Some("missing".to_owned()))
                .with_max_entries(Some(0)),
        )
        .expect("list");
    assert!(
        stream
            .next_entry()
            .expect("zero returned entries fit")
            .is_none()
    );
}

#[test]
fn prefix_does_not_bypass_provider_walker_budget() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("nested")).expect("parent");
    std::fs::write(root.path().join("nested/item"), b"x").expect("child");
    let policy = LocalResourcePolicy::bounded(
        LocalListResourceLimits::new(
            8,
            1,
            4096,
            4,
            std::time::Duration::from_secs(60),
        )
        .expect("list budget"),
        LocalCopyResourceLimits::new(
            8,
            64,
            4096,
            4,
            std::time::Duration::from_secs(60),
        )
        .expect("copy budget"),
        LocalDeleteResourceLimits::new(
            8,
            64,
            4096,
            std::time::Duration::from_secs(60),
        ),
    );
    let fs = LocalFileSystems::rooted(root.path(), policy).expect("rooted");
    for requested in [None, Some(1), Some(100)] {
        let mut stream = fs
            .list(
                &Path::root(),
                ListOptions::default()
                    .with_prefix(Some("nested/item".to_owned()))
                    .with_max_entries(requested),
            )
            .expect("list");
        assert_eq!(
            FsErrorKind::ResourceLimitExceeded,
            stream
                .next_entry()
                .expect_err("provider still counts parent and child")
                .kind()
        );
    }
}

#[test]
fn prefix_return_budget_still_rejects_second_match() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("nested")).expect("parent");
    std::fs::write(root.path().join("nested/item"), b"x").expect("child");
    let fs =
        LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted");
    let mut stream = fs
        .list(
            &Path::root(),
            ListOptions::default()
                .with_prefix(Some("nested".to_owned()))
                .with_max_entries(Some(1)),
        )
        .expect("list");
    assert_eq!(
        Path::parse("/nested").expect("path"),
        stream
            .next_entry()
            .expect("first match")
            .expect("entry")
            .path
    );
    assert_eq!(
        FsErrorKind::ResourceLimitExceeded,
        stream
            .next_entry()
            .expect_err("second match exceeds returned budget")
            .kind()
    );
}
