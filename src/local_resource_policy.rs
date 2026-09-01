// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Explicit recursive resource policy for local filesystem providers.
// qubit-style: allow multiple-public-types

use std::time::Duration;

use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_local_files as native_files;

/// Resource limits applied to recursive local listings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalListResourceLimits {
    max_depth: usize,
    max_entries: usize,
    max_seen_name_bytes: usize,
    max_open_directories: usize,
    deadline: Duration,
}

impl LocalListResourceLimits {
    /// Creates complete recursive listing limits.
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
    pub const fn max_depth(self) -> usize {
        self.max_depth
    }

    /// Returns the maximum number of returned entries.
    #[must_use]
    pub const fn max_entries(self) -> usize {
        self.max_entries
    }

    /// Returns the maximum total bytes retained for seen entry names.
    #[must_use]
    pub const fn max_seen_name_bytes(self) -> usize {
        self.max_seen_name_bytes
    }

    /// Returns the maximum concurrently open directories.
    #[must_use]
    pub const fn max_open_directories(self) -> usize {
        self.max_open_directories
    }

    /// Returns the recursive operation deadline.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Converts these resource-only limits to neutral native list options.
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

/// Resource limits applied to recursive local copies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalCopyResourceLimits {
    max_depth: usize,
    max_entries: usize,
    max_bytes: u64,
    max_open_directories: usize,
    deadline: Duration,
}

impl LocalCopyResourceLimits {
    /// Creates complete recursive copy limits.
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
    pub const fn max_depth(self) -> usize {
        self.max_depth
    }

    /// Returns the maximum number of traversed entries.
    #[must_use]
    pub const fn max_entries(self) -> usize {
        self.max_entries
    }

    /// Returns the maximum copied bytes.
    #[must_use]
    pub const fn max_bytes(self) -> u64 {
        self.max_bytes
    }

    /// Returns the maximum concurrently open directories.
    #[must_use]
    pub const fn max_open_directories(self) -> usize {
        self.max_open_directories
    }

    /// Returns the recursive operation deadline.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Converts these resource-only limits to neutral native copy options.
    pub(crate) const fn native_options(
        self,
    ) -> native_files::options::LocalCopyOptions {
        native_files::options::LocalCopyOptions::new()
            .with_max_depth(self.max_depth)
            .with_max_entries(self.max_entries)
            .with_max_bytes(self.max_bytes)
            .with_max_open_directories(self.max_open_directories)
            .with_deadline(self.deadline)
    }
}

/// Explicit local-provider policy for recursive list and copy resources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalResourcePolicy {
    list: Option<LocalListResourceLimits>,
    copy: Option<LocalCopyResourceLimits>,
}

impl LocalResourcePolicy {
    /// Creates a bounded resource policy.
    #[must_use]
    pub const fn bounded(
        list: LocalListResourceLimits,
        copy: LocalCopyResourceLimits,
    ) -> Self {
        Self {
            list: Some(list),
            copy: Some(copy),
        }
    }

    /// Explicitly opts into unbounded recursive resource usage.
    #[must_use]
    pub const fn unbounded() -> Self {
        Self {
            list: None,
            copy: None,
        }
    }

    /// Returns the listing limits, if recursive listing is bounded.
    #[must_use]
    pub const fn list_limits(self) -> Option<LocalListResourceLimits> {
        self.list
    }

    /// Returns the copy limits, if recursive copying is bounded.
    #[must_use]
    pub const fn copy_limits(self) -> Option<LocalCopyResourceLimits> {
        self.copy
    }

    pub(crate) const fn list_options(
        self,
    ) -> native_files::options::LocalListOptions {
        match self.list {
            Some(limits) => limits.native_options(),
            None => native_files::options::LocalListOptions::new(),
        }
    }

    pub(crate) const fn copy_options(
        self,
    ) -> native_files::options::LocalCopyOptions {
        match self.copy {
            Some(limits) => limits.native_options(),
            None => native_files::options::LocalCopyOptions::new(),
        }
    }
}

fn invalid_open_directories() -> FsError {
    FsError::new(
        FsErrorKind::InvalidOptions,
        FsOperation::Provider,
        "max_open_directories must be greater than zero",
    )
}
