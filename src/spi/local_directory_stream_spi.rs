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

use super::error_mapper::LocalFileErrorMapper;
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
            let entry = entry.map_err(entry_error)?;
            let logical_relative =
                LocalPathMapper::rooted_logical(entry.relative_path())?;
            if !options.matches(&logical_relative) {
                continue;
            }
            let path =
                output_path(rooted.as_ref(), &logical_relative, entry.path())?;
            let mut result =
                DirEntry::new(path, output_kind(entry.metadata().kind()));
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

fn entry_error(error: native_files::LocalFileError) -> FsError {
    LocalFileErrorMapper::map_without_path(
        error,
        FsOperation::List,
        "native directory walk failed",
    )
}

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
        None => LocalPathMapper::host_logical(native),
    }
}

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

#[cfg(test)]
mod tests {
    use std::path::Path as NativePath;

    use qubit_fs::{
        FileKind,
        FsErrorKind,
        Path,
    };
    use qubit_local_files::{
        LocalFileError,
        LocalFileErrorKind,
        LocalFileKind,
        LocalFileOperation,
    };

    use super::{
        ListingOptions,
        entry_error,
        output_kind,
        output_path,
    };

    #[test]
    fn listing_options_apply_exact_and_descendant_prefixes() {
        let options = ListingOptions {
            include_metadata: true,
            prefix: Some("nested/item".to_owned()),
        };
        assert!(options.include_metadata);
        for path in ["/nested/item", "/nested/item/child"] {
            assert!(
                options
                    .matches(&Path::parse(path).expect("test path must parse"))
            );
        }
        for path in ["/nested/items", "/other"] {
            assert!(
                !options
                    .matches(&Path::parse(path).expect("test path must parse"))
            );
        }
    }

    #[test]
    fn listing_options_without_prefix_match_every_entry() {
        let options = ListingOptions {
            include_metadata: false,
            prefix: None,
        };
        assert!(!options.include_metadata);
        assert!(
            options.matches(
                &Path::parse("/anything").expect("test path must parse")
            )
        );
    }

    #[test]
    fn maps_host_and_rooted_entry_paths() {
        let relative = Path::parse("/child").expect("test path must parse");
        let root = Path::parse("/root").expect("test path must parse");
        assert_eq!(
            root,
            output_path(Some(&root), &Path::root(), NativePath::new("ignored"))
                .expect("root relative path must select request root")
        );
        assert_eq!(
            relative,
            output_path(
                Some(&Path::root()),
                &relative,
                NativePath::new("ignored")
            )
            .expect("rooted root must preserve logical relative path")
        );
        assert_eq!(
            Path::parse("/root/child").expect("test path must parse"),
            output_path(Some(&root), &relative, NativePath::new("ignored"))
                .expect("nested rooted path must join request root")
        );
        assert_eq!(
            Path::parse("/tmp/child").expect("test path must parse"),
            output_path(None, &relative, NativePath::new("/tmp/child"))
                .expect("host path must map from native path")
        );
    }

    #[test]
    fn maps_entry_kinds_and_walk_errors() {
        assert_eq!(FileKind::File, output_kind(LocalFileKind::File));
        assert_eq!(FileKind::Directory, output_kind(LocalFileKind::Directory));
        assert_eq!(FileKind::Symlink, output_kind(LocalFileKind::Symlink));
        assert_eq!(
            FileKind::Other("local".to_owned()),
            output_kind(LocalFileKind::Other)
        );
        let error = entry_error(LocalFileError::new(
            LocalFileErrorKind::Io,
            LocalFileOperation::List,
        ));
        assert_eq!(FsErrorKind::Io, error.kind());
    }
}
