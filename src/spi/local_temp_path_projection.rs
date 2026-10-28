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
    /// Absolute namespace parent supplied by the portable caller, retaining
    /// aliases rather than resolving them through the filesystem.
    requested_parent: PathBuf,
    /// Resolved creation parent retained by the native resource.
    native_parent: PathBuf,
}

impl LocalTempPathProjection {
    /// Captures the requested and native creation parents.
    ///
    /// No filesystem lookup or authority change occurs.
    ///
    /// # Parameters
    ///
    /// - `requested_parent`: Absolute namespace parent supplied by the portable
    ///   caller, retaining its requested alias spelling.
    /// - `native_parent`: Native namespace parent used to create the resource.
    ///
    /// # Returns
    ///
    /// A projection pairing the requested parent with the native creation
    /// parent.
    pub(crate) fn new(requested_parent: &Path, native_parent: &Path) -> Self {
        Self {
            requested_parent: requested_parent.to_path_buf(),
            native_parent: native_parent.to_path_buf(),
        }
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

    /// Projection accepts native layouts with arbitrary names and depth.
    #[test]
    fn test_projection_accepts_arbitrary_native_layout() {
        let projection = LocalTempPathProjection::new(Path::new("alias"), Path::new("real"));
        let projected = projection
            .project(
                Path::new("real/private-v2/nested/file"),
                FsOperation::CreateTemp,
            )
            .expect("native layout under explicit parent projects successfully");
        assert_eq!(Path::new("alias/private-v2/nested/file"), projected);
    }

    /// Keep output must remain under the captured native creation parent.
    #[test]
    fn test_projection_rejects_foreign_keep_parent() {
        let projection = LocalTempPathProjection::new(Path::new("alias"), Path::new("real"));
        let error = projection
            .project(Path::new("foreign/kept"), FsOperation::KeepTemp)
            .expect_err("foreign output must be rejected");
        assert_eq!(error.kind(), FsErrorKind::ProviderContractViolation);
    }
}
