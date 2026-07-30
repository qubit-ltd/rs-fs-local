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

/// Converts an absolute logical path to a process-host native path.
///
/// # Parameters
///
/// - `path`: Absolute logical path to convert.
///
/// # Returns
///
/// The equivalent native host path.
///
/// # Errors
///
/// Returns `InvalidPath` when `path` is relative or a component cannot be
/// represented by the native path layer.
#[inline]
pub(crate) fn host(path: &Path) -> FsResult<PathBuf> {
    require_absolute(path)?;
    native_files::LocalPaths::from_canonical_absolute_components(
        std::iter::once("").chain(path.components()).collect(),
    )
    .map_err(|error| map(error, path, FsOperation::ParsePath))
}

/// Converts an absolute logical path to a rooted-authority relative path.
///
/// # Parameters
///
/// - `path`: Absolute logical path within a rooted filesystem.
///
/// # Returns
///
/// An empty path for the logical root or a relative native descendant path.
///
/// # Errors
///
/// Returns `InvalidPath` when `path` is relative or a component cannot be
/// represented by the native path layer.
pub(crate) fn rooted(path: &Path) -> FsResult<PathBuf> {
    require_absolute(path)?;
    let components: Vec<_> = path.components().collect();
    if components.is_empty() {
        Ok(PathBuf::new())
    } else {
        native_files::LocalPaths::from_canonical_relative_components(components)
            .map_err(|error| map(error, path, FsOperation::ParsePath))
    }
}

/// Converts a source-target pair to process-host native paths.
///
/// # Parameters
///
/// - `source`: Absolute logical source path.
/// - `target`: Absolute logical target path.
///
/// # Returns
///
/// The converted native source and target paths in the same order.
///
/// # Errors
///
/// Returns `InvalidPath` when either logical path is relative or contains an
/// unrepresentable component. Any error retains the failing logical path.
#[inline(always)]
pub(crate) fn host_pair(
    source: &Path,
    target: &Path,
) -> FsResult<(PathBuf, PathBuf)> {
    Ok((host(source)?, host(target)?))
}

/// Converts a source-target pair to rooted-authority native paths.
///
/// # Parameters
///
/// - `source`: Absolute logical source path within the rooted authority.
/// - `target`: Absolute logical target path within the rooted authority.
///
/// # Returns
///
/// The converted relative native source and target paths in the same order.
///
/// # Errors
///
/// Returns `InvalidPath` when either logical path is relative or contains an
/// unrepresentable component. Any error retains the failing logical path.
#[inline(always)]
pub(crate) fn rooted_pair(
    source: &Path,
    target: &Path,
) -> FsResult<(PathBuf, PathBuf)> {
    Ok((rooted(source)?, rooted(target)?))
}

/// Converts an absolute process-host native path to a logical path.
///
/// # Parameters
///
/// - `path`: Absolute native host path to convert.
///
/// # Returns
///
/// The canonical absolute logical path.
///
/// # Errors
///
/// Returns `InvalidPath` when the native path is not absolute or cannot be
/// represented in canonical logical form.
///
/// # Panics
///
/// Panics if `qubit-local-files` violates its contract by returning an
/// absolute component sequence without a root component.
pub(crate) fn host_logical(path: &NativePath) -> FsResult<Path> {
    let components =
        native_files::LocalPaths::to_canonical_absolute_components(path)
            .map_err(|error| map_native(error, FsOperation::List))?;
    let (root, rest) = components
        .split_first()
        .expect("LocalPaths returns an absolute canonical path with its root");
    debug_assert!(
        root.is_empty(),
        "LocalPaths absolute canonical paths begin with an empty root"
    );
    logical(rest)
}

/// Converts a rooted-authority relative native path to a logical path.
///
/// # Parameters
///
/// - `path`: Relative native descendant path, or an empty path for the root.
///
/// # Returns
///
/// The canonical absolute logical path inside the rooted authority.
///
/// # Errors
///
/// Returns `InvalidPath` when the native path is not a canonical relative
/// path or cannot be represented in logical form.
#[inline]
pub(crate) fn rooted_logical(path: &NativePath) -> FsResult<Path> {
    if path.as_os_str().is_empty() {
        return Ok(Path::root());
    }
    let components =
        native_files::LocalPaths::to_canonical_relative_components(path)
            .map_err(|error| map_native(error, FsOperation::List))?;
    logical(&components)
}

/// Ensures a facade path is absolute before native conversion.
///
/// # Parameters
///
/// - `path`: Logical path to validate.
///
/// # Returns
///
/// `Ok(())` when `path` is absolute.
///
/// # Errors
///
/// Returns `InvalidPath` when `path` is relative.
#[inline]
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

/// Builds a canonical logical path from decoded components.
///
/// # Parameters
///
/// - `components`: Canonical component text without the logical root marker.
///
/// # Returns
///
/// The parsed absolute logical path.
///
/// # Errors
///
/// Returns `InvalidPath` when the joined canonical text cannot be parsed.
#[inline]
fn logical(components: &[String]) -> FsResult<Path> {
    Path::parse(&format!("/{}", components.join("/")))
}

/// Attaches logical-path context to a native conversion failure.
///
/// # Parameters
///
/// - `error`: Native conversion failure.
/// - `path`: Logical path whose conversion failed.
/// - `operation`: Facade operation performing the conversion.
///
/// # Returns
///
/// An `InvalidPath` facade error retaining `path` and the native source.
#[inline(always)]
fn map(
    error: native_files::LocalFileError,
    path: &Path,
    operation: FsOperation,
) -> FsError {
    map_native(error, operation).with_path(path.clone())
}

/// Maps a native conversion failure without inventing logical-path context.
///
/// # Parameters
///
/// - `error`: Native conversion failure.
/// - `operation`: Facade operation performing the conversion.
///
/// # Returns
///
/// An `InvalidPath` facade error retaining the native source.
#[inline(always)]
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
