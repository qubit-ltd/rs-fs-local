// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public behavior coverage for retained directory-listing options.

use qubit_fs::directory::CreateDirectoryOptions;
use qubit_fs::directory::ListFilter;
use qubit_fs::directory::ListOptions;
use qubit_fs::error::FsErrorKind;
use qubit_fs::path::Path;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalCopyResourceLimits;
use qubit_fs_local::LocalDeleteResourceLimits;
use qubit_fs_local::LocalDirectoryReopenPolicy;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalListResourceLimits;
use qubit_fs_local::LocalResourcePolicy;

/// Builds a complete provider policy around the listing limits under test.
fn bounded_list_policy(
    max_entries: usize,
    max_open_directories: usize,
    reopen_policy: LocalDirectoryReopenPolicy,
) -> LocalResourcePolicy {
    LocalResourcePolicy::bounded(
        LocalListResourceLimits::new(
            8,
            max_entries,
            4096,
            max_open_directories,
            std::time::Duration::from_secs(60),
        )
        .expect("list budget"),
        LocalCopyResourceLimits::new(8, 64, 4096, 4, std::time::Duration::from_secs(60)).expect("copy budget"),
        LocalDeleteResourceLimits::new(8, 64, 4096, std::time::Duration::from_secs(60)),
    )
    .with_directory_reopen_policy(reopen_policy)
}

/// Prefix filtering includes the exact subtree and preserves requested
/// metadata without exposing sibling entries.
#[test]
fn test_listing_options_filter_prefix_and_include_metadata() {
    let root = tempfile::tempdir().expect("listing root must be created");
    let file_system =
        LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted filesystem must open");
    let list_root = Path::parse("/reports").expect("listing path must be valid");
    let matching_directory = Path::parse("/reports/nested").expect("matching path must be valid");
    let matching_file = Path::parse("/reports/nested/report.txt").expect("matching file path must be valid");
    let sibling_directory = Path::parse("/reports/nested-other").expect("sibling path must be valid");
    let sibling_file = Path::parse("/reports/nested-other/report.txt").expect("sibling file path must be valid");

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
    while let Some(entry) = stream.next_entry().expect("prefix-filtered listing must not fail") {
        assert!(
            entry.metadata.is_some(),
            "requested metadata must be present for {}",
            entry.path
        );
        entries.push(entry.path.as_str().to_owned());
    }
    entries.sort();

    assert_eq!(
        vec!["/reports/nested".to_owned(), "/reports/nested/report.txt".to_owned(),],
        entries
    );
}

#[test]
fn test_prefix_request_budget_counts_only_matching_entries() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("nested")).expect("parent");
    std::fs::write(root.path().join("nested/item"), b"x").expect("child");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
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
fn test_prefix_zero_return_budget_allows_no_matches() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("unmatched"), b"x").expect("fixture");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    let mut stream = fs
        .list(
            &Path::root(),
            ListOptions::default()
                .with_prefix(Some("missing".to_owned()))
                .with_max_entries(Some(0)),
        )
        .expect("list");
    assert!(stream.next_entry().expect("zero returned entries fit").is_none());
}

#[test]
fn test_prefix_does_not_bypass_provider_walker_budget() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("nested")).expect("parent");
    std::fs::write(root.path().join("nested/item"), b"x").expect("child");
    let policy = bounded_list_policy(1, 4, LocalDirectoryReopenPolicy::Reopen);
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
        let error = stream.next_entry().expect_err("provider still counts parent and child");
        assert_eq!(FsErrorKind::ResourceLimitExceeded, error.kind(),);
        assert_eq!(Some(&Path::root()), error.path());
        assert_eq!(Some("local-file"), error.provider());
    }
}

#[test]
fn test_prefix_return_budget_still_rejects_second_match() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("nested")).expect("parent");
    std::fs::write(root.path().join("nested/item"), b"x").expect("child");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
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
        stream.next_entry().expect("first match").expect("entry").path
    );
    let error = stream.next_entry().expect_err("second match exceeds returned budget");
    assert_eq!(FsErrorKind::ResourceLimitExceeded, error.kind(),);
    assert_eq!(Some(&Path::root()), error.path());
    assert_eq!(Some("local-file"), error.provider());
}

/// Literal-prefix listing is rejected for hierarchical local path semantics.
#[test]
fn test_literal_prefix_is_rejected_with_request_context() {
    let root = tempfile::tempdir().expect("root");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    let logical_root = Path::root();

    let error = fs
        .list(
            &logical_root,
            ListOptions::default().with_filter(Some(ListFilter::LiteralPrefix("partial".to_owned()))),
        )
        .expect_err("literal prefix requires flat path semantics");

    assert_eq!(FsErrorKind::InvalidOptions, error.kind());
    assert_eq!(Some(&logical_root), error.path());
    assert_eq!(Some("local-file"), error.provider());
}

/// An unmatched prefix exhausts the provider walker without consuming the
/// caller return budget.
#[test]
fn test_unmatched_prefix_exhausts_without_return_budget_error() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("archive")).expect("directory");
    std::fs::write(root.path().join("archive/item"), b"x").expect("fixture");
    let fs = LocalFileSystems::rooted(
        root.path(),
        bounded_list_policy(2, 4, LocalDirectoryReopenPolicy::Reopen),
    )
    .expect("rooted");
    let mut stream = fs
        .list(
            &Path::root(),
            ListOptions::default()
                .with_prefix(Some("missing".to_owned()))
                .with_max_entries(Some(0)),
        )
        .expect("list");

    assert!(stream.next_entry().expect("no matching entries").is_none());
}

/// Removing a discovered directory before lazy descent preserves list
/// request context on the traversal failure.
#[test]
fn test_midstream_directory_failure_preserves_context() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("nested")).expect("directory");
    std::fs::write(root.path().join("nested/item"), b"x").expect("fixture");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    let logical_root = Path::root();
    let mut stream = fs
        .list(&logical_root, ListOptions::default().with_recursive(true))
        .expect("list");

    assert_eq!(
        Path::parse("/nested").expect("path"),
        stream
            .next_entry()
            .expect("directory discovery")
            .expect("directory entry")
            .path,
    );
    std::fs::remove_dir_all(root.path().join("nested")).expect("concurrent removal");
    let error = stream
        .next_entry()
        .expect_err("lazy descent must observe the removed directory");

    assert_eq!(FsErrorKind::NotFound, error.kind());
    assert_eq!(Some(&logical_root), error.path());
    assert_eq!(Some("local-file"), error.provider());
}

/// Fail reopen policy reports the provider handle budget after yielding the
/// discovered child directory.
#[test]
fn test_fail_reopen_policy_preserves_context() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("nested")).expect("directory");
    std::fs::write(root.path().join("nested/item"), b"x").expect("fixture");
    let fs = LocalFileSystems::rooted(root.path(), bounded_list_policy(8, 1, LocalDirectoryReopenPolicy::Fail))
        .expect("rooted");
    let logical_root = Path::root();
    let mut stream = fs
        .list(&logical_root, ListOptions::default().with_recursive(true))
        .expect("list");

    assert_eq!(
        Path::parse("/nested").expect("path"),
        stream
            .next_entry()
            .expect("directory discovery")
            .expect("directory entry")
            .path,
    );
    let error = stream
        .next_entry()
        .expect_err("Fail policy must reject reopening beyond the handle cap");

    assert_eq!(FsErrorKind::ResourceLimitExceeded, error.kind());
    assert_eq!(Some(&logical_root), error.path());
    assert_eq!(Some("local-file"), error.provider());
}
