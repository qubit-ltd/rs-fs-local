// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Eager descriptor-relative directory enumeration.

use std::{
    path::PathBuf,
    vec,
};

use qubit_fs::{
    DirEntry,
    DirectoryStreamSession,
    FsError,
    FsErrorKind,
    FsOperation,
    FsPath,
    FsResult,
    ListOptions,
    NativePathCodec,
    OsStrPathCodec,
};
use qubit_local_files::rooted;

use crate::{
    LocalFileSystem,
    RootedLocalFileSystem,
};

/// Directory stream backed by a deterministic rooted snapshot.
pub(crate) struct RootedDirectoryStreamSession {
    /// Remaining entries sorted by canonical provider path.
    entries: vec::IntoIter<DirEntry>,
}

impl RootedDirectoryStreamSession {
    /// Captures a rooted listing without resolving through the diagnostic path.
    pub(crate) fn capture(
        root: &rooted::Root,
        provider_root: &FsPath,
        options: ListOptions,
    ) -> FsResult<Self> {
        if options.follow_symlinks {
            return Err(FsError::new(
                FsErrorKind::UnsupportedCapability,
                FsOperation::List,
                "rooted listing does not follow symbolic links",
            )
            .with_path(provider_root.clone())
            .with_provider(LocalFileSystem::provider_id()));
        }
        let start = if provider_root.as_str() == "/" {
            None
        } else {
            Some(RootedLocalFileSystem::relative_path(
                provider_root,
                FsOperation::List,
            )?)
        };
        let mut pending = vec![(start, provider_root.clone())];
        let mut entries = Vec::new();
        while let Some((relative_directory, provider_directory)) = pending.pop()
        {
            let native_entries = match relative_directory.as_ref() {
                Some(path) => root.read_dir(path),
                None => root.read_root_dir(),
            }
            .map_err(|error| {
                RootedLocalFileSystem::map_io_error(
                    FsOperation::List,
                    &provider_directory,
                    error,
                )
            })?;
            for native_entry in native_entries {
                let name = OsStrPathCodec
                    .decode(native_entry.name())
                    .map_err(|error| {
                        FsError::with_source(
                            FsErrorKind::InvalidPath,
                            FsOperation::List,
                            "native rooted entry name cannot be encoded losslessly",
                            error,
                        )
                        .with_path(provider_directory.clone())
                        .with_provider(LocalFileSystem::provider_id())
                    })?
                    .into_owned();
                let provider_path =
                    child_provider_path(&provider_directory, &name)?;
                let relative_path = child_rooted_path(
                    relative_directory.as_ref(),
                    native_entry.name(),
                );
                let metadata = RootedLocalFileSystem::map_metadata(
                    native_entry.metadata(),
                );
                let matches_prefix = matches_relative_prefix(
                    provider_root,
                    &provider_path,
                    options.prefix.as_deref(),
                );
                if matches_prefix {
                    entries.push(DirEntry {
                        path: provider_path.clone(),
                        name,
                        kind: metadata.kind.clone(),
                        metadata: options.include_metadata.then_some(metadata),
                    });
                }
                if options.recursive
                    && native_entry.metadata().kind()
                        == rooted::EntryKind::Directory
                {
                    pending.push((Some(relative_path), provider_path));
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

impl DirectoryStreamSession for RootedDirectoryStreamSession {
    #[inline]
    fn next_entry(&mut self) -> FsResult<Option<DirEntry>> {
        Ok(self.entries.next())
    }
}

/// Builds one canonical child path.
fn child_provider_path(parent: &FsPath, name: &str) -> FsResult<FsPath> {
    let text = if parent.as_str() == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", parent.as_str())
    };
    FsPath::parse(&text)
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

/// Builds one validated rooted child path.
fn child_rooted_path(
    parent: Option<&rooted::Path>,
    name: &std::ffi::OsStr,
) -> rooted::Path {
    let mut path =
        parent.map_or_else(PathBuf::new, |value| value.as_path().to_path_buf());
    path.push(name);
    rooted::Path::new(path)
        .expect("joining rooted directory entry components stays valid")
}
