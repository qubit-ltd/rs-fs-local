// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_fs_local::LocalTempCleanupControl;
use qubit_fs_local::LocalTempCleanupResourceLimits;

#[test]
fn test_control_clone_observes_explicit_budget_replacement() {
    let limits = LocalTempCleanupResourceLimits::new(4, 100, 4096, Duration::from_secs(1));
    let control = LocalTempCleanupControl::new(Some(limits));
    let shared = control.clone();
    assert_eq!(shared.limits(), Some(limits));
    shared.set_limits(None);
    assert_eq!(control.limits(), None);
    assert_eq!(limits.max_depth(), 4);
    assert_eq!(limits.max_entries(), 100);
    assert_eq!(limits.max_pending_path_bytes(), 4096);
    assert_eq!(limits.deadline(), Duration::from_secs(1));
}

#[test]
fn test_independent_controls_do_not_share_limits() {
    let a = LocalTempCleanupResourceLimits::new(1, 2, 3, Duration::from_secs(4));
    let b = LocalTempCleanupResourceLimits::new(5, 6, 7, Duration::from_secs(8));
    let first = LocalTempCleanupControl::new(Some(a));
    let second = LocalTempCleanupControl::new(Some(b));
    assert_eq!(first.limits(), Some(a));
    assert_eq!(second.limits(), Some(b));
    first.set_limits(None);
    assert_eq!(first.limits(), None);
    assert_eq!(second.limits(), Some(b));
    first.set_limits(Some(a));
    second.set_limits(None);
    assert_eq!(first.limits(), Some(a));
    assert_eq!(second.limits(), None);
}

#[test]
fn test_control_accepts_none_and_some_replacements() {
    let control = LocalTempCleanupControl::new(None);
    let limits = LocalTempCleanupResourceLimits::new(1, 2, 3, Duration::from_secs(4));
    control.set_limits(Some(limits));
    assert_eq!(control.limits(), Some(limits));
    control.set_limits(None);
    assert_eq!(control.limits(), None);
}

#[test]
fn test_control_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LocalTempCleanupControl>();
}
