// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_fs_local::LocalTempCleanupResourceLimits;

#[test]
fn test_resource_limits_preserve_all_dimensions() {
    let limits = LocalTempCleanupResourceLimits::new(4, 100, 4096, Duration::from_secs(1));
    assert_eq!(limits.max_depth(), 4);
    assert_eq!(limits.max_entries(), 100);
    assert_eq!(limits.max_pending_path_bytes(), 4096);
    assert_eq!(limits.deadline(), Duration::from_secs(1));
}

#[test]
fn test_resource_limits_accept_zero_values() {
    let limits = LocalTempCleanupResourceLimits::new(0, 0, 0, Duration::ZERO);
    assert_eq!(limits, LocalTempCleanupResourceLimits::new(0, 0, 0, Duration::ZERO));
}
