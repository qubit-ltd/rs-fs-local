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
            std::iter::once("").chain(path.components()),
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
        let Some((root, rest)) = components.split_first() else {
            return Err(Self::invalid());
        };
        if !root.is_empty() {
            return Err(Self::invalid());
        }
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

    fn invalid() -> FsError {
        FsError::new(
            FsErrorKind::ProviderContractViolation,
            FsOperation::ParsePath,
            "native path has no supported canonical absolute shape",
        )
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
