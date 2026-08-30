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

use std::path::PathBuf;

use qubit_fs::error::FsError;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_fs::metadata::DirEntry;
use qubit_fs::metadata::FileKind;
use qubit_fs::spi::DirectoryStreamSpi;
use qubit_fs::spi::ResolvedListOptions;
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
        /// Provider identity attached to lazy traversal failures.
        String,
    ),
    /// Walks entries below one logical path in a rooted filesystem.
    Rooted(
        /// Native lazy walker that owns the underlying directory traversal.
        native_files::LocalDirectoryWalker,
        /// Facade filtering and metadata behavior retained for the stream.
        ListingOptions,
        /// Provider identity attached to lazy traversal failures.
        String,
    ),
}

impl LocalDirectoryStreamSpi {
    /// Creates a stream that maps entries through the process host namespace.
    ///
    /// # Parameters
    ///
    /// - `walker`: Native lazy directory walker.
    /// - `options`: Resolved listing behavior retained for the stream.
    /// - `provider_id`: Provider identity attached to lazy traversal errors.
    ///
    /// # Returns
    ///
    /// A host-mode directory stream.
    #[inline]
    pub(crate) fn host(
        walker: native_files::LocalDirectoryWalker,
        options: &ResolvedListOptions,
        provider_id: &str,
    ) -> Self {
        Self::Host(walker, ListingOptions::new(options), provider_id.to_owned())
    }

    /// Creates a stream that maps entries below a rooted logical path.
    ///
    /// # Parameters
    ///
    /// - `walker`: Native lazy directory walker.
    /// - `options`: Resolved listing behavior retained for the stream.
    /// - `provider_id`: Provider identity attached to lazy traversal errors.
    ///
    /// # Returns
    ///
    /// A rooted-mode directory stream.
    #[inline]
    pub(crate) fn rooted(
        walker: native_files::LocalDirectoryWalker,
        options: &ResolvedListOptions,
        provider_id: &str,
    ) -> Self {
        Self::Rooted(
            walker,
            ListingOptions::new(options),
            provider_id.to_owned(),
        )
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
            let (entry, scope, options, provider_id) = match self {
                Self::Host(walker, options, provider_id) => (
                    walker.next(),
                    native_files::LocalFileSystemScope::Host,
                    options,
                    provider_id,
                ),
                Self::Rooted(walker, options, provider_id) => (
                    walker.next(),
                    native_files::LocalFileSystemScope::Rooted,
                    options,
                    provider_id,
                ),
            };
            let Some(entry) = entry else {
                return Ok(None);
            };
            let entry =
                entry.map_err(|error| entry_error(error, provider_id))?;
            let logical_relative = local_path_mapper::logical(
                native_files::LocalFileSystemScope::Rooted,
                &PathBuf::from(std::path::MAIN_SEPARATOR_STR)
                    .join(entry.relative_path()),
                FsOperation::List,
            )?;
            if !options.matches(&logical_relative) {
                continue;
            }
            let path = local_path_mapper::logical(
                scope,
                entry.path(),
                FsOperation::List,
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
/// - `provider_id`: Provider identity attached to the mapped failure.
///
/// # Returns
///
/// A facade listing error with local provider context.
#[inline(always)]
fn entry_error(
    error: native_files::LocalFileError,
    provider_id: &str,
) -> FsError {
    error_mapper::map_without_path(
        error,
        FsOperation::List,
        "native directory walk failed",
        provider_id,
    )
}

/// Converts a native file kind to its facade representation.
///
/// # Parameters
///
/// - `kind`: Native file kind observed by the directory walker.
///
/// # Returns
///
/// The equivalent facade kind; platform-specific kinds use a `local-*`
/// `Other` name.
fn output_kind(kind: native_files::LocalFileKind) -> FileKind {
    match kind {
        native_files::LocalFileKind::File => FileKind::File,
        native_files::LocalFileKind::Directory => FileKind::Directory,
        native_files::LocalFileKind::Symlink => FileKind::Symlink,
        native_files::LocalFileKind::Fifo => {
            FileKind::Other("local-fifo".to_owned())
        }
        native_files::LocalFileKind::Socket => {
            FileKind::Other("local-socket".to_owned())
        }
        native_files::LocalFileKind::BlockDevice => {
            FileKind::Other("local-block-device".to_owned())
        }
        native_files::LocalFileKind::CharDevice => {
            FileKind::Other("local-char-device".to_owned())
        }
        native_files::LocalFileKind::Other => {
            FileKind::Other("local".to_owned())
        }
        _ => FileKind::Other("local".to_owned()),
    }
}
