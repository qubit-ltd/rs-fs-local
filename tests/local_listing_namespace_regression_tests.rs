// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
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
        .list(&ListScope::Path((request).clone()), ListOptions::default())
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
