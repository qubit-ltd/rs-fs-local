// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Native error translation with public request context.

use qubit_fs::{
    FsError,
    FsErrorKind,
    FsOperation,
    Path,
};
use qubit_local_files as native_files;

pub(crate) enum LocalFileErrorMapper {}

impl LocalFileErrorMapper {
    pub(crate) fn map(
        error: native_files::LocalFileError,
        operation: FsOperation,
        path: &Path,
        target: Option<&Path>,
    ) -> FsError {
        let kind = match error.kind() {
            native_files::LocalFileErrorKind::InvalidInput => {
                FsErrorKind::InvalidPath
            }
            native_files::LocalFileErrorKind::NotFound => FsErrorKind::NotFound,
            native_files::LocalFileErrorKind::AlreadyExists => {
                FsErrorKind::AlreadyExists
            }
            native_files::LocalFileErrorKind::TypeConflict => {
                FsErrorKind::Conflict
            }
            native_files::LocalFileErrorKind::PermissionDenied => {
                FsErrorKind::PermissionDenied
            }
            native_files::LocalFileErrorKind::Unsupported => {
                FsErrorKind::UnsupportedOperation
            }
            native_files::LocalFileErrorKind::RequirementNotMet => {
                FsErrorKind::RequirementNotMet
            }
            native_files::LocalFileErrorKind::ResourceLimit => {
                FsErrorKind::ResourceLimitExceeded
            }
            native_files::LocalFileErrorKind::PublicationIncomplete => {
                FsErrorKind::Conflict
            }
            native_files::LocalFileErrorKind::Indeterminate => {
                FsErrorKind::Indeterminate
            }
            native_files::LocalFileErrorKind::Io => FsErrorKind::Io,
            _ => FsErrorKind::Other,
        };
        let error = FsError::with_source(
            kind,
            operation,
            "local filesystem operation failed",
            error,
        )
        .with_path(path.clone())
        .with_provider("local-file");
        match target {
            Some(target) => error.with_target(target.clone()),
            None => error,
        }
    }
}

#[cfg(test)]
mod tests {
    use qubit_fs::{
        FsErrorKind,
        FsOperation,
        Path,
    };
    use qubit_local_files::{
        LocalFileError,
        LocalFileErrorKind as Kind,
        LocalFileOperation,
    };

    use super::LocalFileErrorMapper;

    #[test]
    fn maps_native_error_kinds_with_paths() {
        let path = Path::parse("/source").expect("test path must parse");
        let target = Path::parse("/target").expect("test path must parse");
        for (native, expected) in [
            (Kind::InvalidInput, FsErrorKind::InvalidPath),
            (Kind::NotFound, FsErrorKind::NotFound),
            (Kind::AlreadyExists, FsErrorKind::AlreadyExists),
            (Kind::TypeConflict, FsErrorKind::Conflict),
            (Kind::PermissionDenied, FsErrorKind::PermissionDenied),
            (Kind::Unsupported, FsErrorKind::UnsupportedOperation),
            (Kind::RequirementNotMet, FsErrorKind::RequirementNotMet),
            (Kind::ResourceLimit, FsErrorKind::ResourceLimitExceeded),
            (Kind::PublicationIncomplete, FsErrorKind::Conflict),
            (Kind::Indeterminate, FsErrorKind::Indeterminate),
            (Kind::Io, FsErrorKind::Io),
        ] {
            let error = LocalFileErrorMapper::map(
                LocalFileError::new(native, LocalFileOperation::Metadata),
                FsOperation::Stat,
                &path,
                Some(&target),
            );
            assert_eq!(expected, error.kind());
            assert_eq!(Some(&path), error.path());
            assert_eq!(Some(&target), error.target());
        }
    }
}
