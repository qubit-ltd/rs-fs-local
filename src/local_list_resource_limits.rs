// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resource limits for recursive local listings.

use std::time::Duration;

use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_local_files as native_files;

/// Resource limits applied to recursive local listings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalListResourceLimits {
    /// Maximum recursive depth, with the request root at depth zero.
    max_depth: usize,
    /// Maximum entries yielded before adapter filtering.
    max_entries: usize,
    /// Maximum encoded bytes retained for seen entry names.
    max_seen_name_bytes: usize,
    /// Maximum concurrently open directories during traversal.
    max_open_directories: usize,
    /// Cooperative deadline checked between native operations.
    deadline: Duration,
}

impl LocalListResourceLimits {
    /// Creates complete recursive listing limits.
    ///
    /// # Parameters
    ///
    /// - `max_depth`: Maximum recursive depth, starting at zero.
    /// - `max_entries`: Maximum native entries before adapter filtering.
    /// - `max_seen_name_bytes`: Maximum encoded bytes retained for names.
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
    /// use qubit_fs_local::LocalListResourceLimits;
    ///
    /// let limits = LocalListResourceLimits::new(8, 1_000, 4_096, 8, Duration::from_secs(30))?;
    /// assert_eq!(limits.max_open_directories(), 8);
    /// # Ok::<(), qubit_fs::error::FsError>(())
    /// ```
    pub fn new(
        max_depth: usize,
        max_entries: usize,
        max_seen_name_bytes: usize,
        max_open_directories: usize,
        deadline: Duration,
    ) -> FsResult<Self> {
        if max_open_directories == 0 {
            return Err(invalid_open_directories());
        }
        Ok(Self {
            max_depth,
            max_entries,
            max_seen_name_bytes,
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

    /// Returns the maximum entries yielded by the native walker before
    /// adapter prefix filtering.
    #[must_use]
    #[inline(always)]
    pub const fn max_entries(self) -> usize {
        self.max_entries
    }

    /// Returns the maximum total bytes retained for seen entry names.
    #[must_use]
    #[inline(always)]
    pub const fn max_seen_name_bytes(self) -> usize {
        self.max_seen_name_bytes
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

    /// Converts these resource-only limits to neutral native list options.
    #[cfg_attr(debug_assertions, inline(never))]
    pub(crate) const fn native_options(
        self,
    ) -> native_files::options::LocalListOptions {
        native_files::options::LocalListOptions::new()
            .with_max_depth(self.max_depth)
            .with_max_entries(self.max_entries)
            .with_max_seen_name_bytes(self.max_seen_name_bytes)
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
