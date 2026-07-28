// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Eager host-local directory enumeration.

use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    vec,
};

use qubit_fs::{
    DirEntry,
    DirectoryStreamSession,
    FileKind,
    FsOperation,
    FsPath,
    FsResult,
    ListOptions,
};
use qubit_local_files::backend::directory;

use crate::LocalFileSystem;

/// Directory stream backed by a deterministic snapshot of native entries.
pub(crate) struct LocalDirectoryStreamSession {
    /// Remaining entries sorted by canonical provider path.
    entries: vec::IntoIter<DirEntry>,
}

impl LocalDirectoryStreamSession {
    /// Captures a directory listing beneath one host-local path.
    ///
    /// # Parameters
    ///
    /// * `native_root` - Native directory from which enumeration starts.
    /// * `provider_root` - Canonical provider path for error context.
    /// * `options` - Recursion, symbolic-link, metadata, and prefix policies.
    ///
    /// # Returns
    ///
    /// A deterministic stream containing the matching entries.
    ///
    /// # Errors
    ///
    /// Returns a path-aware filesystem error when a directory or entry cannot
    /// be read, metadata cannot be loaded, or a native path cannot be encoded.
    pub(crate) fn capture(
        native_root: PathBuf,
        provider_root: &FsPath,
        options: ListOptions,
    ) -> FsResult<Self> {
        let mut entries = Vec::new();
        let mut pending = vec![native_root];
        let mut visited = HashSet::new();
        if options.follow_symlinks {
            let identity = fs::canonicalize(&pending[0]).map_err(|error| {
                LocalFileSystem::map_io_error(
                    FsOperation::List,
                    provider_root,
                    error,
                )
            })?;
            visited.insert(identity);
        }
        while let Some(directory_path) = pending.pop() {
            let directory_entries =
                directory::read(&directory_path).map_err(|error| {
                    LocalFileSystem::map_io_error(
                        FsOperation::List,
                        provider_root,
                        error,
                    )
                })?;
            for native_entry in directory_entries {
                let native_entry = native_entry.map_err(|error| {
                    LocalFileSystem::map_io_error(
                        FsOperation::List,
                        provider_root,
                        error,
                    )
                })?;
                let native_path = native_entry.path();
                let provider_path = LocalFileSystem::path_from_native(
                    &native_path,
                )
                .map_err(|error| {
                    error
                        .with_operation(FsOperation::List)
                        .with_path(provider_root.clone())
                        .with_provider(LocalFileSystem::provider_id())
                })?;
                let native_metadata = if options.follow_symlinks {
                    native_entry.metadata()
                } else {
                    fs::symlink_metadata(&native_path)
                }
                .map_err(|error| {
                    LocalFileSystem::map_io_error(
                        FsOperation::List,
                        &provider_path,
                        error,
                    )
                })?;
                let metadata = LocalFileSystem::map_metadata(native_metadata);
                let is_directory = metadata.kind == FileKind::Directory;
                let name = provider_path
                    .file_name()
                    .expect("directory entries must have a final component")
                    .to_owned();
                let matches_prefix = matches_relative_prefix(
                    provider_root,
                    &provider_path,
                    options.prefix.as_deref(),
                );
                if matches_prefix {
                    entries.push(DirEntry {
                        path: provider_path,
                        name,
                        kind: metadata.kind.clone(),
                        metadata: options.include_metadata.then_some(metadata),
                    });
                }
                if options.recursive && is_directory {
                    if options.follow_symlinks {
                        let identity = fs::canonicalize(&native_path).map_err(
                            |error| {
                                LocalFileSystem::map_io_error(
                                    FsOperation::List,
                                    provider_root,
                                    error,
                                )
                            },
                        )?;
                        if visited.insert(identity) {
                            pending.push(native_path);
                        }
                    } else {
                        pending.push(native_path);
                    }
                }
            }
        }
        entries
            .sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
        Ok(Self {
            entries: entries.into_iter(),
        })
    }
}

/// Checks a list filter against the entry path relative to the requested root.
fn matches_relative_prefix(
    root: &FsPath,
    entry: &FsPath,
    prefix: Option<&str>,
) -> bool {
    prefix.is_none_or(|prefix| {
        entry
            .as_str()
            .strip_prefix(root.as_str())
            .and_then(|relative| relative.strip_prefix('/').or(Some(relative)))
            .is_some_and(|relative| relative.starts_with(prefix))
    })
}

impl DirectoryStreamSession for LocalDirectoryStreamSession {
    /// Returns the next captured directory entry.
    ///
    /// # Returns
    ///
    /// `Some` for the next entry or `None` after the snapshot is exhausted.
    ///
    /// # Errors
    ///
    /// This eager session reports capture failures from [`Self::capture`] and
    /// therefore does not produce deferred errors.
    #[inline]
    fn next_entry(&mut self) -> FsResult<Option<DirEntry>> {
        Ok(self.entries.next())
    }
}
