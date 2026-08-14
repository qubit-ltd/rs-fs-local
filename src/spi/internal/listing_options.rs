// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resolved listing behavior retained by a directory stream.

use qubit_fs::Path;
use qubit_fs::spi::ResolvedListOptions;

/// Facade listing semantics applied to entries yielded by native I/O.
#[must_use]
pub(in crate::spi) struct ListingOptions {
    /// Whether each already-observed entry metadata snapshot is exposed.
    include_metadata: bool,
    /// Optional slash-separated prefix relative to the requested list root.
    prefix: Option<String>,
}

impl ListingOptions {
    /// Captures resolved facade options for one stream lifetime.
    ///
    /// # Parameters
    ///
    /// - `options`: Resolved listing options supplied by the facade.
    ///
    /// # Returns
    ///
    /// An owned snapshot suitable for retaining with a directory stream.
    #[inline]
    pub(in crate::spi) fn new(options: &ResolvedListOptions) -> Self {
        Self {
            include_metadata: options.options().include_metadata(),
            prefix: options.options().prefix().map(str::to_owned),
        }
    }

    /// Reports whether entry metadata should be included in stream output.
    ///
    /// # Returns
    ///
    /// `true` when each entry should retain its observed metadata snapshot.
    #[inline(always)]
    #[must_use]
    pub(in crate::spi) const fn include_metadata(&self) -> bool {
        self.include_metadata
    }

    /// Tests a canonical relative logical path against the configured prefix.
    ///
    /// # Parameters
    ///
    /// - `relative`: Canonical path relative to the requested listing root.
    ///
    /// # Returns
    ///
    /// `true` when no prefix is configured or the path is the prefix itself
    /// or one of its descendants.
    #[inline]
    #[must_use]
    pub(in crate::spi) fn matches(&self, relative: &Path) -> bool {
        self.prefix.as_ref().is_none_or(|prefix| {
            let relative =
                relative.as_str().strip_prefix('/').unwrap_or_default();
            relative == *prefix
                || relative
                    .strip_prefix(prefix)
                    .is_some_and(|remaining| remaining.starts_with('/'))
        })
    }
}
