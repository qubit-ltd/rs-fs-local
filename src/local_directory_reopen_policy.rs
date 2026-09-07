// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Directory reopen behavior for recursive local walkers.

/// Behavior used when a recursive walker must close and later revisit a
/// directory after reaching its open-handle budget.
///
/// # Examples
///
/// ```
/// use qubit_fs_local::LocalDirectoryReopenPolicy;
/// let policy = LocalDirectoryReopenPolicy::default();
/// assert_eq!(policy, LocalDirectoryReopenPolicy::Reopen);
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalDirectoryReopenPolicy {
    /// Fail instead of reopening directories.
    Fail,
    /// Reopen directories and verify that traversal state remains valid.
    #[default]
    Reopen,
}
