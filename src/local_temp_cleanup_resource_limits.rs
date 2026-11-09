// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Independent resource ceilings for cleaning up local temporary directories.

use std::time::Duration;

use qubit_local_files::options::LocalTempCleanupLimits;

/// Resource ceilings applied to one local temporary directory cleanup attempt.
///
/// The root is at depth zero and counts as an entry. Pending-path bytes measure
/// queued native paths rather than total memory use. The deadline is
/// cooperative and is checked between native operations; it cannot interrupt
/// a native operation already in progress. Zero values are valid and may stop
/// cleanup before it removes entries. Replace limits explicitly when recovery
/// from a budget error is appropriate.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use qubit_fs_local::LocalTempCleanupResourceLimits;
///
/// let limits = LocalTempCleanupResourceLimits::new(8, 1_000, 4_096, Duration::from_secs(30));
/// assert_eq!(limits.max_entries(), 1_000);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalTempCleanupResourceLimits {
    /// Maximum descendant depth; the cleanup root has depth zero.
    max_depth: usize,
    /// Maximum number of discovered entries, including the root.
    max_entries: usize,
    /// Maximum encoded bytes held by queued native paths.
    max_pending_path_bytes: usize,
    /// Cooperative time budget for one cleanup attempt.
    deadline: Duration,
}

impl LocalTempCleanupResourceLimits {
    /// Creates cleanup ceilings. `max_depth` starts at zero for the directory
    /// root, and `max_entries` includes that root. Pending-path bytes count
    /// queued native paths, not total memory use. The deadline is cooperative.
    /// Every field may be zero, which can prevent cleanup work; callers that
    /// need recovery can explicitly replace the limits.
    ///
    /// # Parameters
    ///
    /// - `max_depth`: Maximum descendant depth, with the root at zero.
    /// - `max_entries`: Maximum discovered entries, including the root.
    /// - `max_pending_path_bytes`: Maximum encoded bytes held by queued paths.
    /// - `deadline`: Cooperative duration budget for the cleanup attempt.
    #[must_use]
    pub const fn new(max_depth: usize, max_entries: usize, max_pending_path_bytes: usize, deadline: Duration) -> Self {
        Self {
            max_depth,
            max_entries,
            max_pending_path_bytes,
            deadline,
        }
    }

    /// Returns the maximum descendant depth, with the temporary root at zero.
    #[must_use]
    pub const fn max_depth(self) -> usize {
        self.max_depth
    }

    /// Returns the maximum discovered entries, including the root.
    #[must_use]
    pub const fn max_entries(self) -> usize {
        self.max_entries
    }

    /// Returns the maximum encoded bytes held by queued cleanup paths.
    #[must_use]
    pub const fn max_pending_path_bytes(self) -> usize {
        self.max_pending_path_bytes
    }

    /// Returns the cooperative deadline for one cleanup attempt.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Converts these ceilings to the native options type without performing
    /// I/O.
    pub(crate) const fn native_limits(self) -> LocalTempCleanupLimits {
        LocalTempCleanupLimits::new()
            .with_max_depth(self.max_depth)
            .with_max_entries(self.max_entries)
            .with_max_pending_path_bytes(self.max_pending_path_bytes)
            .with_deadline(self.deadline)
    }
}
