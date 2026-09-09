// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resource limits for recursive local copies.

use std::time::Duration;

use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_local_files as native_files;

/// Resource limits applied to recursive local copies.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use qubit_fs_local::LocalCopyResourceLimits;
/// let limits = LocalCopyResourceLimits::new(4, 100, 4096, 4, Duration::from_secs(5))?;
/// assert_eq!(limits.max_bytes(), 4096);
/// # Ok::<(), qubit_fs::error::FsError>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalCopyResourceLimits {
    /// Maximum recursive depth, with the request root at depth zero.
    max_depth: usize,
    /// Maximum number of entries traversed by one native copy.
    max_entries: usize,
    /// Maximum bytes copied by one native recursive operation.
    max_bytes: u64,
    /// Maximum concurrently open directories during traversal.
    max_open_directories: usize,
    /// Cooperative deadline checked between native operations.
    deadline: Duration,
}

impl LocalCopyResourceLimits {
    /// Creates complete recursive copy limits.
    ///
    /// # Parameters
    ///
    /// - `max_depth`: Maximum recursive depth, starting at zero.
    /// - `max_entries`: Maximum traversed entries.
    /// - `max_bytes`: Maximum bytes copied.
    /// - `max_open_directories`: Positive open-directory budget.
    /// - `deadline`: Cooperative elapsed-time limit.
    ///
    /// # Errors
    ///
    /// Returns an invalid-options error when `max_open_directories` is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use qubit_fs_local::LocalCopyResourceLimits;
    ///
    /// let limits = LocalCopyResourceLimits::new(8, 1_000, 1 << 20, 8, Duration::from_secs(30))?;
    /// assert_eq!(limits.max_bytes(), 1 << 20);
    /// # Ok::<(), qubit_fs::error::FsError>(())
    /// ```
    pub fn new(
        max_depth: usize,
        max_entries: usize,
        max_bytes: u64,
        max_open_directories: usize,
        deadline: Duration,
    ) -> FsResult<Self> {
        if max_open_directories == 0 {
            return Err(invalid_open_directories());
        }
        Ok(Self {
            max_depth,
            max_entries,
            max_bytes,
            max_open_directories,
            deadline,
        })
    }

    /// Returns the maximum recursive depth.
    #[must_use]
    #[inline(always)]
    pub const fn max_depth(self) -> usize {
        self.max_depth
    }

    /// Returns the maximum number of traversed entries.
    #[must_use]
    #[inline(always)]
    pub const fn max_entries(self) -> usize {
        self.max_entries
    }

    /// Returns the maximum copied bytes.
    #[must_use]
    #[inline(always)]
    pub const fn max_bytes(self) -> u64 {
        self.max_bytes
    }

    /// Returns the maximum concurrently open directories.
    #[must_use]
    #[inline(always)]
    pub const fn max_open_directories(self) -> usize {
        self.max_open_directories
    }

    /// Returns the recursive operation deadline.
    #[must_use]
    #[inline(always)]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Converts these resource-only limits to neutral native copy options.
    #[cfg_attr(debug_assertions, inline(never))]
    pub(crate) const fn native_options(self) -> native_files::options::LocalCopyOptions {
        native_files::options::LocalCopyOptions::new()
            .with_max_depth(self.max_depth)
            .with_max_entries(self.max_entries)
            .with_max_bytes(self.max_bytes)
            .with_max_open_directories(self.max_open_directories)
            .with_deadline(self.deadline)
    }
}

/// Creates the error returned when no directory handle may be opened.
///
/// The zero handle budget is rejected before any native I/O occurs.
fn invalid_open_directories() -> FsError {
    FsError::new(
        FsErrorKind::InvalidOptions,
        FsOperation::Provider,
        "max_open_directories must be greater than zero",
    )
}
