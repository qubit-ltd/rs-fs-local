// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Explicit recursive resource policy for local filesystem providers.

use std::num::NonZeroUsize;
use std::time::Duration;

use qubit_local_files as native_files;

use crate::LocalCopyResourceLimits;
use crate::LocalDeleteResourceLimits;
use crate::LocalDirectoryReopenPolicy;
use crate::LocalListResourceLimits;

/// Per-request provider ceilings for recursive local operations.
///
/// Provider ceilings bound native operation work. A caller's listing limit
/// counts entries returned after adapter filtering; copy limits and other
/// comparable request limits may only tighten provider ceilings. Deletion
/// limits apply only to ordinary recursive deletion and are selected
/// independently. Temporary-resource cleanup, `Drop`, and native publication
/// cleanup have separate lifecycle semantics and do not inherit deletion
/// ceilings. These are not aggregate quotas across concurrent requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalResourcePolicy {
    list: Option<LocalListResourceLimits>,
    copy: Option<LocalCopyResourceLimits>,
    /// Independently selected ceilings for ordinary recursive deletion
    /// requests. Temporary-resource cleanup, `Drop`, and native publication
    /// cleanup use separate lifecycle semantics.
    delete: Option<LocalDeleteResourceLimits>,
    open_retry_timeout: Option<Duration>,
    temp_max_attempts: Option<NonZeroUsize>,
    directory_reopen_policy: LocalDirectoryReopenPolicy,
}

impl LocalResourcePolicy {
    /// Sets finite per-request ceilings for listing, copying, and deletion.
    ///
    /// This does not enable recursive operation modes. Temporary-resource
    /// cleanup, `Drop`, and native publication cleanup have separate lifecycle
    /// semantics and do not inherit these operation ceilings. The ceilings are
    /// not aggregate quotas across concurrent requests.
    ///
    /// # Parameters
    ///
    /// - `list`: Limits on native listing work before adapter prefix filtering.
    /// - `copy`: Limits on each native copy operation.
    /// - `delete`: Limits on each ordinary recursive deletion operation.
    ///
    /// # Returns
    ///
    /// A policy retaining all three explicit operation ceilings.
    #[must_use]
    pub const fn bounded(
        list: LocalListResourceLimits,
        copy: LocalCopyResourceLimits,
        delete: LocalDeleteResourceLimits,
    ) -> Self {
        Self {
            list: Some(list),
            copy: Some(copy),
            delete: Some(delete),
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

    /// Returns the optional per-request ordinary recursive deletion ceilings.
    ///
    /// Temporary-resource cleanup, `Drop`, and native publication cleanup do
    /// not use these ceilings.
    #[must_use]
    pub const fn delete_limits(self) -> Option<LocalDeleteResourceLimits> {
        self.delete
    }

    /// Selects ordinary recursive deletion ceilings; `None` explicitly leaves
    /// ordinary deletion unbounded.
    ///
    /// Temporary-resource cleanup, `Drop`, and native publication cleanup do
    /// not use these ceilings.
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
