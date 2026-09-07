// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resolved listing behavior retained by a directory stream.

use qubit_fs::directory::ListFilter;
use qubit_fs::path::Path;
use qubit_fs::spi::ResolvedListOptions;

/// Facade listing semantics applied to entries yielded by native I/O.
#[must_use]
pub(in crate::spi) struct ListingOptions {
    /// Whether each already-observed entry metadata snapshot is exposed.
    include_metadata: bool,
    /// Explicit facade filter retained for post-walk matching.
    filter: Option<ListFilter>,
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
            filter: options.options().filter().cloned(),
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
    /// `true` when the path satisfies the configured filter.
    #[inline]
    #[must_use]
    pub(in crate::spi) fn matches(&self, relative: &Path) -> bool {
        match self.filter.as_ref() {
            None => true,
            Some(ListFilter::Subtree(prefix)) => {
                let relative =
                    relative.as_str().strip_prefix('/').unwrap_or_default();
                relative == prefix
                    || relative
                        .strip_prefix(prefix)
                        .is_some_and(|remaining| remaining.starts_with('/'))
            }
            Some(ListFilter::LiteralPrefix(prefix)) => relative
                .as_str()
                .strip_prefix('/')
                .unwrap_or_default()
                .starts_with(prefix),
        }
    }
}

#[cfg(test)]
mod tests {
    use qubit_fs::directory::ListFilter;
    use qubit_fs::path::Path;

    use super::ListingOptions;

    /// Literal prefixes use raw key text rather than component boundaries.
    #[test]
    fn test_literal_prefix_matches_partial_components() {
        let options = ListingOptions {
            include_metadata: false,
            filter: Some(ListFilter::LiteralPrefix("report".to_owned())),
        };

        assert!(
            options.matches(
                &Path::parse("/reports/annual.txt")
                    .expect("matching relative logical path"),
            )
        );
        assert!(
            !options.matches(
                &Path::parse("/archive/report.txt")
                    .expect("non-matching relative logical path"),
            )
        );
    }
}
