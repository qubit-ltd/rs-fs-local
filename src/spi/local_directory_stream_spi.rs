// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Lazy directory walker adapter.

use std::path::Path as NativePath;

use qubit_fs::spi::{
    DirectoryStreamSpi,
    ResolvedListOptions,
};
use qubit_fs::{
    DirEntry,
    FsError,
    FsOperation,
    FsResult,
    Path,
};
use qubit_local_files as native_files;

use super::error_mapper;
use super::internal::ListingOptions;
use super::local_outcome_mapper;
use crate::path::local_path_mapper;

/// Lazily maps a native directory walker into facade directory entries.
#[must_use]
pub(crate) enum LocalDirectoryStreamSpi {
    /// Walks entries in the process host namespace.
    Host(
        /// Native lazy walker that owns the underlying directory traversal.
        native_files::LocalDirectoryWalker,
        /// Facade filtering and metadata behavior retained for the stream.
        ListingOptions,
    ),
    /// Walks entries below one logical path in a rooted filesystem.
    Rooted(
        /// Native lazy walker that owns the underlying directory traversal.
        native_files::LocalDirectoryWalker,
        /// Logical path used as the root for returned entries.
        Path,
        /// Facade filtering and metadata behavior retained for the stream.
        ListingOptions,
    ),
}

impl LocalDirectoryStreamSpi {
    /// Creates a stream that maps entries through the process host namespace.
    ///
    /// # Parameters
    ///
    /// - `walker`: Native lazy directory walker.
    /// - `options`: Resolved listing behavior retained for the stream.
    ///
    /// # Returns
    ///
    /// A host-mode directory stream.
    #[inline]
    pub(crate) fn host(
        walker: native_files::LocalDirectoryWalker,
        options: &ResolvedListOptions,
    ) -> Self {
        Self::Host(walker, ListingOptions::new(options))
    }

    /// Creates a stream that maps entries below a rooted logical path.
    ///
    /// # Parameters
    ///
    /// - `walker`: Native lazy directory walker.
    /// - `root`: Logical path requested by the caller.
    /// - `options`: Resolved listing behavior retained for the stream.
    ///
    /// # Returns
    ///
    /// A rooted-mode directory stream.
    #[inline]
    pub(crate) fn rooted(
        walker: native_files::LocalDirectoryWalker,
        root: Path,
        options: &ResolvedListOptions,
    ) -> Self {
        Self::Rooted(walker, root, ListingOptions::new(options))
    }
}

impl DirectoryStreamSpi for LocalDirectoryStreamSpi {
    /// Advances the native walker until one entry passes facade filtering.
    ///
    /// # Returns
    ///
    /// `Some(entry)` for the next matching entry or `None` after the walker is
    /// exhausted.
    ///
    /// # Errors
    ///
    /// Returns an error when native walking fails or an entry path cannot be
    /// represented as a canonical logical path.
    fn next_entry(&mut self) -> FsResult<Option<DirEntry>> {
        loop {
            let (entry, rooted, options) = match self {
                Self::Host(walker, options) => (walker.next(), None, options),
                Self::Rooted(walker, root, options) => {
                    (walker.next(), Some(root.clone()), options)
                }
            };
            let Some(entry) = entry else {
                return Ok(None);
            };
            let entry = entry.map_err(entry_error)?;
            let logical_relative =
                local_path_mapper::rooted_logical(entry.relative_path())?;
            if !options.matches(&logical_relative) {
                continue;
            }
            let path = output_path(
                rooted.as_ref(),
                &logical_relative,
                entry.diagnostic_path(),
            )?;
            let mut result =
                DirEntry::new(path, output_kind(entry.metadata().kind()));
            if options.include_metadata() {
                result.metadata = Some(local_outcome_mapper::metadata(
                    entry.metadata().clone(),
                ));
            }
            return Ok(Some(result));
        }
    }
}

/// Maps a lazy native walker failure without inventing a logical entry path.
///
/// # Parameters
///
/// - `error`: Native directory-walk failure.
///
/// # Returns
///
/// A facade listing error with local provider context.
#[inline(always)]
fn entry_error(error: native_files::LocalFileError) -> FsError {
    error_mapper::map_without_path(
        error,
        FsOperation::List,
        "native directory walk failed",
    )
}

/// Resolves one native entry path into caller-visible logical output.
///
/// # Parameters
///
/// - `root`: Rooted request path, or `None` for host-mode conversion.
/// - `relative`: Canonical logical path relative to the walker root.
/// - `native`: Native absolute entry path used by host mode.
///
/// # Returns
///
/// The canonical logical path to expose for the entry.
///
/// # Errors
///
/// Returns `InvalidPath` when joining or native-path conversion cannot produce
/// a canonical logical path.
fn output_path(
    root: Option<&Path>,
    relative: &Path,
    native: &NativePath,
) -> FsResult<Path> {
    match root {
        Some(root) if relative == &Path::root() => Ok(root.clone()),
        Some(root) if root == &Path::root() => Ok(relative.clone()),
        Some(root) => Path::parse(&format!(
            "{}/{}",
            root.as_str(),
            &relative.as_str()[1..]
        )),
        None => local_path_mapper::host_logical(native),
    }
}

/// Converts a native file kind to its facade representation.
///
/// # Parameters
///
/// - `kind`: Native file kind observed by the directory walker.
///
/// # Returns
///
/// The equivalent facade kind; platform-specific kinds use `Other("local")`.
fn output_kind(kind: native_files::LocalFileKind) -> qubit_fs::FileKind {
    match kind {
        native_files::LocalFileKind::File => qubit_fs::FileKind::File,
        native_files::LocalFileKind::Directory => qubit_fs::FileKind::Directory,
        native_files::LocalFileKind::Symlink => qubit_fs::FileKind::Symlink,
        native_files::LocalFileKind::Other => {
            qubit_fs::FileKind::Other("local".to_owned())
        }
    }
}
