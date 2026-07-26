// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Hierarchical provider-path validation.

use qubit_fs::{
    FsError,
    FsErrorKind,
    FsOperation,
    FsPath,
    FsResult,
};

use crate::LocalFileSystem;

/// Validates that a provider path is absolute and already normalized.
///
/// Object-key literals may preserve empty, current-directory, and
/// parent-directory components. Hierarchical local filesystems must reject
/// those aliases before native path conversion so resource identity and
/// policy checks use the same path that reaches the operating system.
///
/// # Parameters
///
/// * `operation` - Filesystem operation that is validating the path.
/// * `path` - Provider path supplied by the caller.
///
/// # Returns
///
/// `Ok(())` when `path` is an absolute normalized hierarchical path.
///
/// # Errors
///
/// Returns [`FsErrorKind::InvalidPath`] when the path is relative, escapes
/// above its root, or differs from its normalized hierarchical form.
pub(crate) fn validate_hierarchical_path(
    operation: FsOperation,
    path: &FsPath,
) -> FsResult<()> {
    if !path.is_absolute() {
        return Err(FsError::invalid_path(
            operation,
            "local filesystem path must be absolute",
        )
        .with_path(path.clone())
        .with_provider(LocalFileSystem::provider_id()));
    }
    let normalized =
        FsPath::parse_normalized(path.as_str()).map_err(|error| {
            FsError::with_source(
                FsErrorKind::InvalidPath,
                operation,
                "local filesystem path must be normalized",
                error,
            )
            .with_path(path.clone())
            .with_provider(LocalFileSystem::provider_id())
        })?;
    if normalized != *path {
        return Err(FsError::invalid_path(
            operation,
            "local filesystem path must not contain hierarchical aliases",
        )
        .with_path(path.clone())
        .with_provider(LocalFileSystem::provider_id()));
    }
    Ok(())
}
