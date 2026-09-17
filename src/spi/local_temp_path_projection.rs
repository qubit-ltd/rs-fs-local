// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public temporary paths retain the requested parent without changing
//! ownership.

use std::path::Path;
use std::path::PathBuf;

use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;

/// Projects generated paths from the native parent to its requested alias.
#[derive(Debug)]
pub(crate) struct LocalTempPathProjection {
    /// Parent spelling supplied by the portable caller.
    requested_parent: PathBuf,
    /// Resolved creation parent retained by the native resource.
    native_parent: PathBuf,
}

impl LocalTempPathProjection {
    /// Captures the native library's one-private-sandbox creation layout.
    ///
    /// `resource` must contain a generated sandbox and a resource name below
    /// its creation parent. No filesystem lookup or authority change occurs.
    ///
    /// # Parameters
    ///
    /// - `requested_parent`: Parent spelling supplied by the portable caller.
    /// - `resource`: Native path of the generated temporary resource.
    ///
    /// # Returns
    ///
    /// A projection pairing the requested parent with the native creation
    /// parent inferred from `resource`.
    ///
    /// # Errors
    ///
    /// Returns a provider-contract error for an unexpected native layout.
    pub(crate) fn new(requested_parent: &Path, resource: &Path) -> FsResult<Self> {
        let sandbox = resource.parent().filter(|path| {
            path.file_name()
                .is_some_and(|name| name.as_encoded_bytes().starts_with(b"sandbox-"))
        });
        let native_parent = sandbox.and_then(Path::parent).ok_or_else(|| {
            FsError::new(
                FsErrorKind::ProviderContractViolation,
                FsOperation::CreateTemp,
                "native temporary resource has no private creation sandbox",
            )
        })?;
        Ok(Self {
            requested_parent: requested_parent.to_path_buf(),
            native_parent: native_parent.to_path_buf(),
        })
    }

    /// Re-expresses a generated descendant beneath the requested parent.
    ///
    /// # Parameters
    ///
    /// - `path`: Native path of a generated descendant to project.
    /// - `operation`: Facade operation requesting the projection for errors.
    ///
    /// # Returns
    ///
    /// The same relative path expressed under the requested parent spelling.
    ///
    /// # Errors
    ///
    /// Returns a provider-contract error if `path` is outside the captured
    /// native parent.
    pub(crate) fn project(&self, path: &Path, operation: FsOperation) -> FsResult<PathBuf> {
        let relative = path.strip_prefix(&self.native_parent).map_err(|error| {
            FsError::with_source(
                FsErrorKind::ProviderContractViolation,
                operation,
                "generated temporary path escaped its creation parent",
                error,
            )
        })?;
        Ok(self.requested_parent.join(relative))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use qubit_fs::error::FsErrorKind;
    use qubit_fs::error::FsOperation;

    use super::LocalTempPathProjection;

    /// Projection rejects a native layout that cannot establish its parent.
    #[test]
    fn test_projection_rejects_missing_sandbox() {
        let error = LocalTempPathProjection::new(Path::new("alias"), Path::new("real/file"))
            .expect_err("a native sandbox is required");
        assert_eq!(error.kind(), FsErrorKind::ProviderContractViolation);
    }

    /// Keep output must remain under the captured native creation parent.
    #[test]
    fn test_projection_rejects_foreign_keep_parent() {
        let projection = LocalTempPathProjection::new(Path::new("alias"), Path::new("real/sandbox-test/file"))
            .expect("native sandbox layout");
        let error = projection
            .project(Path::new("foreign/kept"), FsOperation::KeepTemp)
            .expect_err("foreign output must be rejected");
        assert_eq!(error.kind(), FsErrorKind::ProviderContractViolation);
    }
}
