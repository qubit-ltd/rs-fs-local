// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Explicit standard policy defaults and independent overrides.

use std::time::Duration;

use qubit_fs::Path;
use qubit_fs::directory::ListScope;
use qubit_fs::read::ReadOptions;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalCopyResourceLimits;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalListResourceLimits;
use qubit_fs_local::LocalResourcePolicy;

/// Defaults are stable public configuration, not hidden ambient settings.
#[test]
fn test_standard_has_exact_finite_limits() {
    let policy = LocalResourcePolicy::standard();
    let list = policy.list_limits().expect("bounded list");
    assert_eq!(list.max_depth(), 64);
    assert_eq!(list.max_entries(), 100_000);
    assert_eq!(list.max_seen_name_bytes(), 16 * 1024 * 1024);
    assert_eq!(list.max_open_directories(), 32);
    assert_eq!(list.deadline(), Duration::from_secs(30));
    let copy = policy.copy_limits().expect("bounded copy");
    assert_eq!(copy.max_depth(), 64);
    assert_eq!(copy.max_entries(), 100_000);
    assert_eq!(copy.max_bytes(), 1024 * 1024 * 1024);
    assert_eq!(copy.max_open_directories(), 32);
    assert_eq!(copy.deadline(), Duration::from_secs(30));
    let delete = policy.delete_limits().expect("bounded delete");
    assert_eq!(delete.max_depth(), 64);
    assert_eq!(delete.max_entries(), 100_000);
    assert_eq!(delete.max_pending_path_bytes(), 16 * 1024 * 1024);
    assert_eq!(delete.deadline(), Duration::from_secs(30));
}

/// Overriding one domain leaves the other configured domains intact.
#[test]
fn test_standard_overrides_are_independent_and_none_is_explicit() {
    let original = LocalResourcePolicy::standard();
    let list = LocalListResourceLimits::new(1, 2, 1024, 1, Duration::from_secs(2)).expect("limits");
    let copy = LocalCopyResourceLimits::new(1, 2, 2048, 1, Duration::from_secs(2)).expect("limits");
    let policy = original.with_list_limits(Some(list));
    assert_eq!(policy.list_limits(), Some(list));
    assert_eq!(policy.copy_limits(), original.copy_limits());
    assert_eq!(policy.delete_limits(), original.delete_limits());
    let policy = policy.with_copy_limits(Some(copy));
    assert_eq!(policy.copy_limits(), Some(copy));
    assert_eq!(policy.list_limits(), Some(list));
    assert_eq!(policy.delete_limits(), original.delete_limits());
    let policy = policy
        .with_list_limits(None)
        .with_copy_limits(None)
        .with_delete_limits(None);
    assert_eq!(policy, LocalResourcePolicy::unbounded());
}

/// Standard policy supports a normal isolated report lifecycle.
#[test]
fn test_standard_rooted_report_and_tightened_list_limit() {
    let root = tempfile::tempdir().expect("isolated root");
    let fs = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::standard()).expect("filesystem");
    let path = Path::parse("/report").expect("path");
    fs.write_all(&path, b"report", WriteOptions::default()).expect("write");
    assert_eq!(fs.read_all(&path, ReadOptions::default(), 6).expect("read"), b"report");
    let scope = ListScope::Path(Path::root());
    assert!(
        fs.list(&scope, Default::default())
            .expect("list")
            .next_entry()
            .expect("entry")
            .is_some()
    );
    let list = LocalListResourceLimits::new(64, 0, 1024, 1, Duration::from_secs(30)).expect("zero entries");
    let fs = LocalFileSystems::rooted(
        root.path(),
        LocalResourcePolicy::standard().with_list_limits(Some(list)),
    )
    .expect("filesystem");
    let mut stream = fs.list(&scope, Default::default()).expect("open list");
    assert!(stream.next_entry().is_err(), "tightened native limit must apply");
}
