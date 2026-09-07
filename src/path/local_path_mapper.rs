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

use std::path::Path as NativePath;
use std::path::PathBuf;

use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_fs::path::Path;
use qubit_local_files as native_files;

/// Converts an absolute logical path to a native path in one authority scope.
///
/// # Parameters
///
/// - `scope`: Native authority in which to interpret the logical path.
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
pub(crate) fn native(
    scope: native_files::path::LocalFileSystemScope,
    path: &Path,
) -> FsResult<PathBuf> {
    require_absolute(path)?;
    let paths = match scope {
        native_files::path::LocalFileSystemScope::Host => native_files::path::LocalPaths::host(),
        native_files::path::LocalFileSystemScope::Rooted => {
            native_files::path::LocalPaths::rooted()
        }
    };
    paths
        .from_canonical_components(path.components())
        .map_err(|error| map(error, path, FsOperation::ParsePath))
}

/// Converts a native path to a logical path in one authority scope.
///
/// # Parameters
///
/// - `scope`: Native authority that owns `path`.
/// - `path`: Native path to convert.
/// - `operation`: Facade operation requesting the conversion.
///
/// # Returns
///
/// The canonical absolute logical path.
///
/// # Errors
///
/// Returns `InvalidPath` when the native path is not absolute or cannot be
/// represented in canonical logical form.
pub(crate) fn logical(
    scope: native_files::path::LocalFileSystemScope,
    path: &NativePath,
    operation: FsOperation,
) -> FsResult<Path> {
    let paths = match scope {
        native_files::path::LocalFileSystemScope::Host => native_files::path::LocalPaths::host(),
        native_files::path::LocalFileSystemScope::Rooted => {
            native_files::path::LocalPaths::rooted()
        }
    };
    let components = paths
        .to_canonical_components(path)
        .map_err(|error| map_native(error, operation))?;
    logical_components(&components)
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
/// - `components`: Canonical component text beneath the logical root.
///
/// # Returns
///
/// The parsed absolute logical path.
///
/// # Errors
///
/// Returns `InvalidPath` when the joined canonical text cannot be parsed.
#[inline]
fn logical_components(components: &[String]) -> FsResult<Path> {
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
fn map(error: native_files::LocalFileError, path: &Path, operation: FsOperation) -> FsError {
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
fn map_native(error: native_files::LocalFileError, operation: FsOperation) -> FsError {
    FsError::with_source(
        FsErrorKind::InvalidPath,
        operation,
        "local native path conversion failed",
        error,
    )
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;
    #[cfg(unix)]
    use std::path::PathBuf;

    use qubit_fs::error::FsErrorKind;
    use qubit_fs::error::FsOperation;
    use qubit_fs::path::Path;
    use qubit_local_files::path::LocalFileSystemScope;

    use super::logical;
    use super::native;

    /// Rooted conversion rejects relative logical paths before native I/O.
    #[test]
    fn test_native_rejects_rooted_relative_logical_path() {
        let path = Path::parse("relative").expect("relative logical path text");

        let error = native(LocalFileSystemScope::Rooted, &path)
            .expect_err("rooted conversion requires an absolute logical path");

        assert_eq!(FsErrorKind::InvalidPath, error.kind());
        assert_eq!(FsOperation::ParsePath, error.operation());
        assert_eq!(None, error.path());
    }

    /// Host conversion preserves non-UTF-8 Unix filename bytes.
    #[cfg(unix)]
    #[test]
    fn test_logical_preserves_absolute_non_utf8_host_component() {
        let mut native_path = PathBuf::from(std::path::MAIN_SEPARATOR_STR);
        native_path.push(OsString::from_vec(vec![b'r', b'a', b'w', 0xff]));

        let logical_path = logical(
            LocalFileSystemScope::Host,
            &native_path,
            FsOperation::ParsePath,
        )
        .expect("absolute non-UTF-8 host path must convert");

        assert_eq!("/raw%FF", logical_path.as_str());
    }
}
