// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::hint::black_box;
use std::num::NonZeroUsize;
use std::time::Duration;

use qubit_fs_local::LocalCopyResourceLimits;
use qubit_fs_local::LocalDirectoryReopenPolicy;
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

#[test]
fn local_execution_controls_are_explicit_and_independent_of_recursion_budgets()
{
    let timeout = Duration::from_millis(250);
    let attempts = NonZeroUsize::new(32).expect("positive attempt count");
    let policy = black_box(
        LocalResourcePolicy::with_open_retry_timeout
            as fn(LocalResourcePolicy, Option<Duration>) -> LocalResourcePolicy,
    )(LocalResourcePolicy::unbounded(), Some(timeout));
    let policy = black_box(
        LocalResourcePolicy::with_temp_max_attempts
            as fn(
                LocalResourcePolicy,
                Option<NonZeroUsize>,
            ) -> LocalResourcePolicy,
    )(policy, Some(attempts));
    let policy = black_box(
        LocalResourcePolicy::with_directory_reopen_policy
            as fn(
                LocalResourcePolicy,
                LocalDirectoryReopenPolicy,
            ) -> LocalResourcePolicy,
    )(policy, LocalDirectoryReopenPolicy::Fail);

    assert_eq!(
        Some(timeout),
        black_box(LocalResourcePolicy::open_retry_timeout)(policy)
    );
    assert_eq!(
        Some(attempts),
        black_box(LocalResourcePolicy::temp_max_attempts)(policy)
    );
    assert_eq!(
        LocalDirectoryReopenPolicy::Fail,
        policy.directory_reopen_policy()
    );
    assert_eq!(None, policy.list_limits());
    assert_eq!(None, policy.copy_limits());
}
