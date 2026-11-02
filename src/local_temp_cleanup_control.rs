// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared, replaceable cleanup limits for associated temporary directories.

use std::sync::Arc;
use std::sync::RwLock;

use crate::LocalTempCleanupResourceLimits;

/// Shared configuration for the next cleanup attempt of associated resources.
///
/// Clones share the same limits. Reading returns a copy, and changing limits
/// does not interrupt an attempt already using its snapshot. `None` means no
/// cleanup-specific limit is applied. A poisoned lock retains and exposes its
/// last value.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use qubit_fs_local::{LocalTempCleanupControl, LocalTempCleanupResourceLimits};
///
/// let limits = LocalTempCleanupResourceLimits::new(8, 1_000, 4_096, Duration::from_secs(30));
/// let control = LocalTempCleanupControl::new(Some(limits));
/// let shared = control.clone();
/// shared.set_limits(None);
/// assert_eq!(control.limits(), None);
/// ```
#[derive(Clone, Debug)]
pub struct LocalTempCleanupControl {
    limits: Arc<RwLock<Option<LocalTempCleanupResourceLimits>>>,
}

impl LocalTempCleanupControl {
    /// Creates a control with the given optional cleanup limits.
    ///
    /// `None` means no cleanup-specific ceilings. Cloning this control shares
    /// its configuration with the clone.
    #[must_use]
    pub fn new(limits: Option<LocalTempCleanupResourceLimits>) -> Self {
        Self {
            limits: Arc::new(RwLock::new(limits)),
        }
    }

    /// Returns a copy of the currently configured limits, or `None` for no
    /// limit.
    #[must_use]
    pub fn limits(&self) -> Option<LocalTempCleanupResourceLimits> {
        *self.limits.read().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Replaces the limits used by subsequent cleanup attempts.
    ///
    /// This does not interrupt attempts already using a snapshot. Passing
    /// `None` explicitly selects unbounded cleanup for later attempts.
    pub fn set_limits(&self, limits: Option<LocalTempCleanupResourceLimits>) {
        *self.limits.write().unwrap_or_else(std::sync::PoisonError::into_inner) = limits;
    }
}
