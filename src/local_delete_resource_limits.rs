// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Explicit provider ceilings for one recursive deletion request.

use std::time::Duration;

use qubit_local_files::options::LocalDeleteOptions;

/// Resource ceilings applied to each recursive local deletion request.
///
/// Entries include the requested directory at depth zero. Pending-path bytes
/// count encoded native path lengths, not allocator overhead. The deadline is
/// cooperative and cannot interrupt an in-flight native call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalDeleteResourceLimits {
    /// Maximum descendant depth.
    max_depth: usize,
    /// Maximum discovered entries, including the requested root.
    max_entries: usize,
    /// Maximum encoded bytes held by pending deletion paths.
    max_pending_path_bytes: usize,
    /// Maximum cooperative elapsed time.
    deadline: Duration,
}

impl LocalDeleteResourceLimits {
    /// Creates complete recursive deletion ceilings.
    ///
    /// Zero values are valid. Depth zero permits only the requested root;
    /// zero entries, path bytes, or elapsed time prevent recursive work.
    ///
    /// # Parameters
    ///
    /// - `max_depth`: Maximum depth, starting at zero for the requested root.
    /// - `max_entries`: Maximum discovered entries, including that root.
    /// - `max_pending_path_bytes`: Maximum encoded path bytes in the work
    ///   queue.
    /// - `deadline`: Maximum elapsed time checked between native calls.
    ///
    /// # Returns
    ///
    /// Independent per-request deletion ceilings for a provider policy.
    #[must_use]
    pub const fn new(
        max_depth: usize,
        max_entries: usize,
        max_pending_path_bytes: usize,
        deadline: Duration,
    ) -> Self {
        Self {
            max_depth,
            max_entries,
            max_pending_path_bytes,
            deadline,
        }
    }

    /// Returns the maximum depth, with the requested root at zero.
    #[must_use]
    pub const fn max_depth(self) -> usize {
        self.max_depth
    }
    /// Returns the maximum discovered entries, including the root.
    #[must_use]
    pub const fn max_entries(self) -> usize {
        self.max_entries
    }
    /// Returns the maximum encoded bytes held by pending paths.
    #[must_use]
    pub const fn max_pending_path_bytes(self) -> usize {
        self.max_pending_path_bytes
    }
    /// Returns the cooperative per-request deadline.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Produces native resource limits without selecting deletion behavior.
    pub(crate) const fn native_options(self) -> LocalDeleteOptions {
        LocalDeleteOptions::new()
            .with_max_depth(self.max_depth)
            .with_max_entries(self.max_entries)
            .with_max_pending_path_bytes(self.max_pending_path_bytes)
            .with_deadline(self.deadline)
    }
}
