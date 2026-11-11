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
#[cfg(windows)]
use qubit_fs::error::FsErrorKind;
use qubit_fs::path::Path;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
#[cfg(windows)]
use qubit_fs_local::host_path_to_logical;

#[cfg(windows)]
#[test]
fn test_windows_host_and_rooted_listing_root_boundaries() {
    let fixture = tempfile::tempdir().expect("fixture root");
    std::fs::write(fixture.path().join("item"), b"x").expect("fixture file");
    let host = LocalFileSystems::host(LocalResourcePolicy::standard()).expect("host");
    let error = host
        .list(&ListScope::Path(Path::root()), ListOptions::default())
        .err()
        .expect("Windows Host has no global root");
    assert_eq!(FsErrorKind::InvalidPath, error.kind());

    let native = fixture.path();
    let host_root = host_path_to_logical(native).expect("convert native host path");
    let rooted = LocalFileSystems::rooted(native, LocalResourcePolicy::standard()).expect("rooted filesystem");
    for (filesystem, root) in [(&host, host_root), (&rooted, Path::root())] {
        let mut stream = filesystem
            .list(&ListScope::Path(root.clone()), ListOptions::default())
            .expect("list explicit filesystem root");
        assert!(stream.next_entry().expect("fixture entry").is_some());
        assert!(stream.next_entry().expect("end of listing").is_none());

        let mut filtered = filesystem
            .list(
                &ListScope::Path(root),
                ListOptions::default().with_prefix(Some("item".to_owned())),
            )
            .expect("Windows subtree prefix should map through rooted codec");
        assert!(
            filtered
                .next_entry()
                .expect("filtered entry")
                .expect("matching file")
                .path
                .as_str()
                .ends_with("/item")
        );
        assert!(filtered.next_entry().expect("filtered listing ends").is_none());
    }
}

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
            for include_metadata in [false, true] {
                let mut stream = filesystem
                    .list(&scope, options.clone().with_include_metadata(include_metadata))
                    .expect("open listing");
                let mut actual = Vec::new();
                while let Some(entry) = stream.next_entry().expect("entry in requested scope") {
                    assert_eq!(
                        entry.metadata.is_some(),
                        include_metadata,
                        "metadata for {}",
                        entry.path
                    );
                    assert_eq!(entry.path.is_absolute(), root.starts_with('/'));
                    actual.push(entry.path.as_str().to_owned());
                }
                actual.sort();
                let mut expected = suffixes
                    .iter()
                    .map(|suffix| format!("{root}/{suffix}"))
                    .collect::<Vec<_>>();
                expected.sort();
                assert_eq!(actual, expected, "scope {root}, metadata={include_metadata}");
            }
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

    let mut filtered = filesystem
        .list(
            &ListScope::Path(Path::parse("base/dir").expect("relative root")),
            ListOptions::default().with_prefix(Some("100%25".to_owned())),
        )
        .expect("percent-encoded subtree prefix must open");
    assert_eq!(
        Some("base/dir/100%25"),
        filtered
            .next_entry()
            .expect("percent-encoded entry is readable")
            .as_ref()
            .map(|entry| entry.path.as_str())
    );
    assert!(filtered.next_entry().expect("filtered listing completes").is_none());
}

#[cfg(unix)]
#[test]
fn test_rooted_subtree_prefix_decodes_non_utf8_components() {
    use std::os::unix::ffi::OsStrExt;

    let fixture = tempfile::tempdir().expect("fixture root");
    std::fs::write(fixture.path().join(std::ffi::OsStr::from_bytes(b"entry-\xff")), b"x").expect("non-UTF-8 entry");
    let filesystem = LocalFileSystems::rooted(fixture.path(), LocalResourcePolicy::unbounded()).expect("rooted");
    let mut stream = filesystem
        .list(
            &ListScope::Path(Path::root()),
            ListOptions::default().with_prefix(Some("entry-%FF".to_owned())),
        )
        .expect("non-UTF-8 subtree prefix must open");
    let entry = stream
        .next_entry()
        .expect("non-UTF-8 target is readable")
        .expect("matching target");
    assert_eq!("/entry-%FF", entry.path.as_str());
    assert!(stream.next_entry().expect("filtered listing completes").is_none());
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

#[cfg(unix)]
#[test]
fn test_host_listing_preserves_non_utf8_entry_names() {
    use std::os::unix::ffi::OsStrExt;

    use qubit_fs_local::host_path_to_logical;

    let fixture = tempfile::tempdir().expect("fixture root");
    let native_path = fixture.path().join(std::ffi::OsStr::from_bytes(b"entry-\xff"));
    std::fs::write(&native_path, b"x").expect("non-UTF-8 entry");
    let filesystem = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host filesystem");
    let root = host_path_to_logical(fixture.path()).expect("host listing root");
    let expected = host_path_to_logical(&native_path).expect("host entry path");
    let mut stream = filesystem
        .list(&ListScope::Path(root), ListOptions::default())
        .expect("open listing");

    let entry = stream
        .next_entry()
        .expect("non-UTF-8 entry should be readable")
        .expect("entry should exist");
    assert_eq!(expected, entry.path);
    assert_eq!(
        Some(1),
        filesystem.stat(&entry.path).expect("returned path is reusable").len()
    );
    assert!(stream.next_entry().expect("listing completes").is_none());
}
