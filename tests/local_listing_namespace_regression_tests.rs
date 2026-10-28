// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regression coverage for local directory listings through aliased namespaces.

use qubit_fs::directory::ListOptions;
use qubit_fs::directory::ListScope;
use qubit_fs::path::Path;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;

/// Relative and absolute rooted scopes retain their request namespace.
#[test]
fn test_rooted_listing_preserves_requested_path_form() {
    let fixture = tempfile::tempdir().expect("fixture root");
    std::fs::create_dir_all(fixture.path().join("dir/nested")).expect("directories");
    std::fs::write(fixture.path().join("dir/item"), b"x").expect("item");
    std::fs::write(fixture.path().join("dir/nested/child"), b"y").expect("child");
    let filesystem =
        LocalFileSystems::rooted(fixture.path(), LocalResourcePolicy::standard()).expect("rooted filesystem");

    for root in ["dir", "/dir"] {
        for (options, suffixes) in [
            (ListOptions::default(), vec!["item", "nested"]),
            (
                ListOptions::default().with_recursive(true),
                vec!["item", "nested", "nested/child"],
            ),
            (
                ListOptions::default().with_prefix(Some("nested".to_owned())),
                vec!["nested", "nested/child"],
            ),
            (
                ListOptions::default().with_recursive(true).with_max_depth(Some(1)),
                vec!["item", "nested"],
            ),
        ] {
            let scope = ListScope::Path(Path::parse(root).expect("logical root"));
            let mut stream = filesystem
                .list(&scope, options.with_include_metadata(true))
                .expect("open listing");
            let mut actual = Vec::new();
            while let Some(entry) = stream.next_entry().expect("entry in requested scope") {
                assert!(entry.metadata.is_some(), "metadata for {}", entry.path);
                assert_eq!(entry.path.is_absolute(), root.starts_with('/'));
                actual.push(entry.path.as_str().to_owned());
            }
            actual.sort();
            let mut expected = suffixes
                .iter()
                .map(|suffix| format!("{root}/{suffix}"))
                .collect::<Vec<_>>();
            expected.sort();
            assert_eq!(actual, expected, "scope {root}");
        }
    }
}

/// Projection retains all root components and canonical percent escapes.
#[test]
fn test_rooted_relative_listing_preserves_components_and_percent_names() {
    let fixture = tempfile::tempdir().expect("fixture root");
    std::fs::create_dir_all(fixture.path().join("base/dir")).expect("directories");
    std::fs::write(fixture.path().join("base/dir/100%"), b"x").expect("percent name");
    let filesystem =
        LocalFileSystems::rooted(fixture.path(), LocalResourcePolicy::standard()).expect("rooted filesystem");
    let mut stream = filesystem
        .list(
            &ListScope::Path(Path::parse("base/dir").expect("relative root")),
            ListOptions::default(),
        )
        .expect("open listing");
    let entry = stream
        .next_entry()
        .expect("valid relative entry")
        .expect("percent entry");
    assert_eq!(entry.path.as_str(), "base/dir/100%25");
    assert_eq!(
        filesystem.stat(&entry.path).expect("returned path is reusable").len(),
        Some(1)
    );
    assert!(stream.next_entry().expect("complete listing").is_none());
}

#[cfg(unix)]
#[test]
fn test_host_alias_listing_stays_inside_requested_logical_root() {
    use std::os::unix::fs::symlink;

    use qubit_fs::directory::ListOptions;
    use qubit_fs::directory::ListScope;
    use qubit_fs_local::LocalFileSystems;
    use qubit_fs_local::LocalResourcePolicy;
    use qubit_fs_local::host_path_to_logical;

    let temp = tempfile::tempdir().expect("fixture root should exist");
    let root = std::fs::canonicalize(temp.path()).expect("fixture root should canonicalize");
    std::fs::create_dir(root.join("real")).expect("real directory should exist");
    std::fs::write(root.join("real/item"), b"x").expect("fixture entry should exist");
    symlink(root.join("real"), root.join("alias")).expect("alias should exist");

    let filesystem =
        LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host filesystem should construct");
    let request = host_path_to_logical(&root.join("alias")).expect("request should convert");
    let expected = host_path_to_logical(&root.join("alias/item")).expect("entry should convert");
    let mut stream = filesystem
        .list(&ListScope::Path(request.clone()), ListOptions::default())
        .expect("alias listing should open");

    assert_eq!(
        expected,
        stream
            .next_entry()
            .expect("entry should be readable")
            .expect("entry should exist")
            .path
    );
    assert!(stream.next_entry().expect("listing should finish").is_none());
}
