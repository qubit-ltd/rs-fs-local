// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Canonical conversion between public logical paths and native paths.

use std::path::{
    Path as NativePath,
    PathBuf,
};

use qubit_fs::{
    FsError,
    FsErrorKind,
    FsOperation,
    FsResult,
    Path,
};
use qubit_local_files as native_files;

pub(crate) enum LocalPathMapper {}

impl LocalPathMapper {
    pub(crate) fn host(path: &Path) -> FsResult<PathBuf> {
        Self::require_absolute(path)?;
        native_files::LocalPaths::from_canonical_absolute_components(
            std::iter::once("").chain(path.components()).collect(),
        )
        .map_err(|error| Self::map(error, path, FsOperation::ParsePath))
    }

    pub(crate) fn rooted(path: &Path) -> FsResult<PathBuf> {
        Self::require_absolute(path)?;
        let components: Vec<_> = path.components().collect();
        if components.is_empty() {
            Ok(PathBuf::new())
        } else {
            native_files::LocalPaths::from_canonical_relative_components(
                components,
            )
            .map_err(|error| Self::map(error, path, FsOperation::ParsePath))
        }
    }

    pub(crate) fn host_pair(
        source: &Path,
        target: &Path,
    ) -> FsResult<(PathBuf, PathBuf)> {
        Ok((Self::host(source)?, Self::host(target)?))
    }

    pub(crate) fn rooted_pair(
        source: &Path,
        target: &Path,
    ) -> FsResult<(PathBuf, PathBuf)> {
        Ok((Self::rooted(source)?, Self::rooted(target)?))
    }

    pub(crate) fn host_logical(path: &NativePath) -> FsResult<Path> {
        let components =
            native_files::LocalPaths::to_canonical_absolute_components(path)
                .map_err(|error| Self::map_native(error, FsOperation::List))?;
        let (root, rest) = components.split_first().expect(
            "LocalPaths returns an absolute canonical path with its root",
        );
        debug_assert!(
            root.is_empty(),
            "LocalPaths absolute canonical paths begin with an empty root"
        );
        Self::logical(rest)
    }

    pub(crate) fn rooted_logical(path: &NativePath) -> FsResult<Path> {
        if path.as_os_str().is_empty() {
            return Ok(Path::root());
        }
        let components =
            native_files::LocalPaths::to_canonical_relative_components(path)
                .map_err(|error| Self::map_native(error, FsOperation::List))?;
        Self::logical(&components)
    }

    fn require_absolute(path: &Path) -> FsResult<()> {
        if path.is_absolute() {
            Ok(())
        } else {
            Err(FsError::invalid_path(
                FsOperation::ParsePath,
                "local filesystem paths must be absolute",
            ))
        }
    }

    fn logical(components: &[String]) -> FsResult<Path> {
        Path::parse(&format!("/{}", components.join("/")))
    }

    fn map(
        error: native_files::LocalFileError,
        path: &Path,
        operation: FsOperation,
    ) -> FsError {
        Self::map_native(error, operation).with_path(path.clone())
    }

    fn map_native(
        error: native_files::LocalFileError,
        operation: FsOperation,
    ) -> FsError {
        FsError::with_source(
            FsErrorKind::InvalidPath,
            operation,
            "local native path conversion failed",
            error,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path as NativePath;

    use qubit_fs::{
        FsErrorKind,
        FsOperation,
        Path,
    };

    use super::LocalPathMapper;

    /// Converts absolute public paths to host-native paths and back.
    #[test]
    fn test_host_converts_absolute_paths_and_pairs() {
        let source = path("/tmp/source file");
        let target = path("/tmp/target file");

        assert_eq!(
            NativePath::new("/tmp/source file"),
            LocalPathMapper::host(&source)
                .expect("absolute host path should convert")
        );
        let (mapped_source, mapped_target) =
            LocalPathMapper::host_pair(&source, &target)
                .expect("absolute host paths should convert as a pair");
        assert_eq!(NativePath::new("/tmp/source file"), mapped_source);
        assert_eq!(NativePath::new("/tmp/target file"), mapped_target);
        assert_eq!(
            source,
            LocalPathMapper::host_logical(NativePath::new("/tmp/source file"))
                .expect("absolute native path should become logical")
        );
    }

    /// Rejects relative paths before native host conversion.
    #[test]
    fn test_host_rejects_relative_paths_and_preserves_pair_context() {
        let relative =
            Path::parse("relative").expect("relative test path must parse");
        let absolute = path("/target");

        let error = LocalPathMapper::host(&relative)
            .expect_err("relative host path must be rejected");
        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::ParsePath, error.operation());

        let error = LocalPathMapper::host_pair(&absolute, &relative)
            .expect_err("relative target in a host pair must be rejected");
        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::ParsePath, error.operation());
    }

    /// Converts rooted logical paths without leaking host-root authority.
    #[test]
    fn test_rooted_converts_root_descendants_and_pairs() {
        let root = Path::root();
        let source = path("/directory/source");
        let target = path("/directory/target");

        assert_eq!(
            NativePath::new(""),
            LocalPathMapper::rooted(&root).expect(
                "rooted logical root should map to empty relative path"
            )
        );
        assert_eq!(
            NativePath::new("directory/source"),
            LocalPathMapper::rooted(&source)
                .expect("rooted descendant should map to relative path")
        );
        let (mapped_source, mapped_target) =
            LocalPathMapper::rooted_pair(&source, &target)
                .expect("rooted descendants should convert as a pair");
        assert_eq!(NativePath::new("directory/source"), mapped_source);
        assert_eq!(NativePath::new("directory/target"), mapped_target);
        assert_eq!(
            root,
            LocalPathMapper::rooted_logical(NativePath::new(""))
                .expect("empty rooted native path should become logical root")
        );
        assert_eq!(
            source,
            LocalPathMapper::rooted_logical(NativePath::new(
                "directory/source"
            ))
            .expect("rooted native descendant should become logical path")
        );
    }

    /// Rejects relative logical values and invalid native shapes with public
    /// path-conversion errors.
    #[test]
    fn test_rooted_and_logical_conversion_reject_invalid_shapes() {
        let relative =
            Path::parse("relative").expect("relative test path must parse");
        let absolute = path("/target");

        let error = LocalPathMapper::rooted(&relative)
            .expect_err("relative rooted path must be rejected");
        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::ParsePath, error.operation());

        let error = LocalPathMapper::rooted_pair(&absolute, &relative)
            .expect_err("relative rooted pair target must be rejected");
        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::ParsePath, error.operation());

        let error = LocalPathMapper::host_logical(NativePath::new("relative"))
            .expect_err("relative host native path must be rejected");
        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::List, error.operation());

        let error = LocalPathMapper::rooted_logical(NativePath::new("."))
            .expect_err("dot rooted native path must be rejected");
        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::List, error.operation());
    }

    /// Retains the public logical path when canonical component decoding fails.
    #[test]
    fn test_host_and_rooted_reject_components_that_decode_to_separators() {
        let invalid = path("/%2F");

        for error in [
            LocalPathMapper::host(&invalid)
                .expect_err("host separator component must be rejected"),
            LocalPathMapper::rooted(&invalid)
                .expect_err("rooted separator component must be rejected"),
        ] {
            assert_eq!(FsErrorKind::InvalidPath, error.kind());
            assert_eq!(FsOperation::ParsePath, error.operation());
            assert_eq!(Some(&invalid), error.path());
        }
    }

    /// Parses a logical test path.
    fn path(value: &str) -> Path {
        Path::parse(value).expect("test logical path must parse")
    }
}
