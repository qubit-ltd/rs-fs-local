// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Explicit recursive resource policy for local filesystem providers.
// qubit-style: allow multiple-public-types

use std::num::NonZeroUsize;
use std::time::Duration;

use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_local_files as native_files;

use crate::LocalDeleteResourceLimits;

/// Behavior used when a recursive walker must close and later revisit a
/// directory after reaching its open-handle budget.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalDirectoryReopenPolicy {
    /// Fail instead of reopening directories.
    Fail,
    /// Reopen directories and verify that traversal state remains valid.
    #[default]
    Reopen,
}

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
    #[cfg_attr(debug_assertions, inline(never))]
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

/// Per-request provider ceilings for recursive local operations.
///
/// Request list/copy limits can only tighten these ceilings. Deletion limits
/// are selected independently with [`Self::with_delete_limits`]. These are
/// not aggregate quotas across concurrent requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalResourcePolicy {
    list: Option<LocalListResourceLimits>,
    copy: Option<LocalCopyResourceLimits>,
    /// Independently selected ceilings for recursive deletion requests.
    delete: Option<LocalDeleteResourceLimits>,
    open_retry_timeout: Option<Duration>,
    temp_max_attempts: Option<NonZeroUsize>,
    directory_reopen_policy: LocalDirectoryReopenPolicy,
}

impl LocalResourcePolicy {
    /// Creates listing and copy ceilings; deletion remains unbounded until
    /// explicitly configured with [`Self::with_delete_limits`].
    #[must_use]
    pub const fn bounded(
        list: LocalListResourceLimits,
        copy: LocalCopyResourceLimits,
    ) -> Self {
        Self {
            list: Some(list),
            copy: Some(copy),
            delete: None,
            open_retry_timeout: None,
            temp_max_attempts: None,
            directory_reopen_policy: LocalDirectoryReopenPolicy::Reopen,
        }
    }

    /// Explicitly opts into unbounded recursive resource usage.
    #[must_use]
    pub const fn unbounded() -> Self {
        Self {
            list: None,
            copy: None,
            delete: None,
            open_retry_timeout: None,
            temp_max_attempts: None,
            directory_reopen_policy: LocalDirectoryReopenPolicy::Reopen,
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

    /// Returns the optional per-request recursive deletion ceilings.
    #[must_use]
    pub const fn delete_limits(self) -> Option<LocalDeleteResourceLimits> {
        self.delete
    }

    /// Selects recursive deletion ceilings; `None` explicitly leaves them
    /// unbounded.
    #[must_use]
    pub const fn with_delete_limits(
        mut self,
        limits: Option<LocalDeleteResourceLimits>,
    ) -> Self {
        self.delete = limits;
        self
    }

    /// Converts deletion ceilings without enabling recursive deletion itself.
    pub(crate) const fn delete_options(
        self,
    ) -> native_files::options::LocalDeleteOptions {
        match self.delete {
            Some(limits) => limits.native_options(),
            None => native_files::options::LocalDeleteOptions::new(),
        }
    }

    /// Returns the local open retry timeout used for readers and writers.
    #[must_use]
    pub const fn open_retry_timeout(self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Returns the maximum number of temporary-name attempts.
    #[must_use]
    pub const fn temp_max_attempts(self) -> Option<NonZeroUsize> {
        self.temp_max_attempts
    }

    /// Returns the directory reopen behavior used by recursive walkers.
    #[must_use]
    pub const fn directory_reopen_policy(self) -> LocalDirectoryReopenPolicy {
        self.directory_reopen_policy
    }

    /// Sets the local open retry timeout used for readers and writers.
    #[must_use]
    pub const fn with_open_retry_timeout(
        mut self,
        timeout: Option<Duration>,
    ) -> Self {
        self.open_retry_timeout = timeout;
        self
    }

    /// Sets the maximum number of temporary-name attempts.
    #[must_use]
    pub const fn with_temp_max_attempts(
        mut self,
        max_attempts: Option<NonZeroUsize>,
    ) -> Self {
        self.temp_max_attempts = max_attempts;
        self
    }

    /// Sets the directory reopen behavior used by recursive walkers.
    #[must_use]
    pub const fn with_directory_reopen_policy(
        mut self,
        policy: LocalDirectoryReopenPolicy,
    ) -> Self {
        self.directory_reopen_policy = policy;
        self
    }

    pub(crate) const fn list_options(
        self,
    ) -> native_files::options::LocalListOptions {
        let options = match self.list {
            Some(limits) => limits.native_options(),
            None => native_files::options::LocalListOptions::new(),
        };
        options.with_reopen_policy(match self.directory_reopen_policy {
            LocalDirectoryReopenPolicy::Fail => {
                native_files::options::LocalDirectoryReopenPolicy::Fail
            }
            LocalDirectoryReopenPolicy::Reopen => {
                native_files::options::LocalDirectoryReopenPolicy::Reopen
            }
        })
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
