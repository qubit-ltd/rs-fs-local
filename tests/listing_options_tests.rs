// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public behavior coverage for retained directory-listing options.

use qubit_fs::CreateDirectoryOptions;
use qubit_fs::ListOptions;
use qubit_fs::Path;
use qubit_fs::WriteOptions;
use qubit_fs_local::LocalFileSystems;

/// Prefix filtering includes the exact subtree and preserves requested
/// metadata without exposing sibling entries.
#[test]
fn test_listing_options_filter_prefix_and_include_metadata() {
    let root = tempfile::tempdir().expect("listing root must be created");
    let file_system = LocalFileSystems::rooted(root.path()).expect("rooted filesystem must open");
    let list_root = Path::parse("/reports").expect("listing path must be valid");
    let matching_directory = Path::parse("/reports/nested").expect("matching path must be valid");
    let matching_file =
        Path::parse("/reports/nested/report.txt").expect("matching file path must be valid");
    let sibling_directory =
        Path::parse("/reports/nested-other").expect("sibling path must be valid");
    let sibling_file =
        Path::parse("/reports/nested-other/report.txt").expect("sibling file path must be valid");

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
