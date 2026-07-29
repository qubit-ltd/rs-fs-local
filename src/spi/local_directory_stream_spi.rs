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

use qubit_fs::spi::{
    DirectoryStreamSpi,
    ResolvedListOptions,
};
use qubit_fs::{
    DirEntry,
    FsError,
    FsErrorKind,
    FsOperation,
    FsResult,
    Path,
};
use qubit_local_files as native_files;

use crate::path::LocalPathMapper;

pub(crate) enum LocalDirectoryStreamSpi {
    Host(native_files::LocalDirectoryWalker, ListingOptions),
    Rooted(native_files::LocalDirectoryWalker, Path, ListingOptions),
}

/// Facade listing semantics that are applied to entries yielded by native I/O.
pub(crate) struct ListingOptions {
    /// Whether each already-observed entry metadata snapshot is exposed.
    include_metadata: bool,
    /// Optional slash-separated prefix relative to the requested list root.
    prefix: Option<String>,
}

impl ListingOptions {
    /// Captures the resolved facade options for one stream lifetime.
    fn new(options: &ResolvedListOptions) -> Self {
        Self {
            include_metadata: options.options().include_metadata,
            prefix: options.options().prefix.clone(),
        }
    }

    /// Reports whether a canonical logical relative path passes the prefix.
    fn matches(&self, relative: &Path) -> bool {
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

impl LocalDirectoryStreamSpi {
    pub(crate) fn host(
        walker: native_files::LocalDirectoryWalker,
        options: &ResolvedListOptions,
    ) -> Self {
        Self::Host(walker, ListingOptions::new(options))
    }
    pub(crate) fn rooted(
        walker: native_files::LocalDirectoryWalker,
        root: Path,
        options: &ResolvedListOptions,
    ) -> Self {
        Self::Rooted(walker, root, ListingOptions::new(options))
    }
}
impl DirectoryStreamSpi for LocalDirectoryStreamSpi {
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
            let entry = entry.map_err(|error| {
                FsError::with_source(
                    FsErrorKind::Io,
                    FsOperation::List,
                    "native directory walk failed",
                    error,
                )
            })?;
            let logical_relative =
                LocalPathMapper::rooted_logical(entry.relative_path())?;
            if !options.matches(&logical_relative) {
                continue;
            }
            let path = if let Some(root) = rooted {
                if logical_relative == Path::root() {
                    root
                } else if root == Path::root() {
                    logical_relative
                } else {
                    Path::parse(&format!(
                        "{}/{}",
                        root.as_str(),
                        &logical_relative.as_str()[1..]
                    ))?
                }
            } else {
                LocalPathMapper::host_logical(entry.path())?
            };
            let mut result = DirEntry::new(
                path,
                match entry.metadata().kind() {
                    native_files::LocalFileKind::File => {
                        qubit_fs::FileKind::File
                    }
                    native_files::LocalFileKind::Directory => {
                        qubit_fs::FileKind::Directory
                    }
                    native_files::LocalFileKind::Symlink => {
                        qubit_fs::FileKind::Symlink
                    }
                    native_files::LocalFileKind::Other => {
                        qubit_fs::FileKind::Other("local".to_owned())
                    }
                },
            );
            if options.include_metadata {
                result.metadata = Some(
                    super::local_outcome_mapper::LocalOutcomeMapper::metadata(
                        entry.metadata().clone(),
                    ),
                );
            }
            return Ok(Some(result));
        }
    }
}
