// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::time::Duration;

use qubit_fs_local::LocalCopyResourceLimits;
use qubit_fs_local::LocalListResourceLimits;
use qubit_fs_local::LocalResourcePolicy;

#[test]
fn bounded_policy_requires_all_recursive_resource_dimensions() {
    let list = LocalListResourceLimits::new(1, 2, 3, 4, Duration::from_secs(5))
        .expect("nonzero open-directory capacity should be valid");
    let copy =
        LocalCopyResourceLimits::new(6, 7, 8, 9, Duration::from_secs(10))
            .expect("nonzero open-directory capacity should be valid");
    let policy = LocalResourcePolicy::bounded(list, copy);
    assert_eq!(policy.list_limits(), Some(list));
    assert_eq!(policy.copy_limits(), Some(copy));
    assert_eq!(list.max_depth(), 1);
    assert_eq!(list.max_entries(), 2);
    assert_eq!(list.max_seen_name_bytes(), 3);
    assert_eq!(list.max_open_directories(), 4);
    assert_eq!(list.deadline(), Duration::from_secs(5));
    assert_eq!(copy.max_depth(), 6);
    assert_eq!(copy.max_entries(), 7);
    assert_eq!(copy.max_bytes(), 8);
    assert_eq!(copy.max_open_directories(), 9);
    assert_eq!(copy.deadline(), Duration::from_secs(10));
    assert!(LocalListResourceLimits::new(1, 2, 3, 0, Duration::ZERO).is_err());
    assert!(LocalCopyResourceLimits::new(1, 2, 3, 0, Duration::ZERO).is_err());
}

#[test]
fn unbounded_policy_is_an_explicit_empty_budget_selection() {
    let policy = LocalResourcePolicy::unbounded();
    assert_eq!(policy.list_limits(), None);
    assert_eq!(policy.copy_limits(), None);
}
