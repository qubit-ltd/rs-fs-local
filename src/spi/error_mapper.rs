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
        let kind = Self::native_kind(error.kind());
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

    /// Maps an I/O failure that has no caller-visible logical path.
    pub(crate) fn map_io(
        error: std::io::Error,
        operation: FsOperation,
        message: &'static str,
    ) -> FsError {
        FsError::with_source(
            Self::io_kind(error.kind()),
            operation,
            message,
            error,
        )
        .with_provider("local-file")
    }

    /// Maps a native local-files failure without inventing a logical path.
    pub(crate) fn map_without_path(
        error: native_files::LocalFileError,
        operation: FsOperation,
        message: &'static str,
    ) -> FsError {
        FsError::with_source(
            Self::native_kind(error.kind()),
            operation,
            message,
            error,
        )
        .with_provider("local-file")
    }

    /// Converts native local-files error kinds to facade error kinds.
    fn native_kind(kind: native_files::LocalFileErrorKind) -> FsErrorKind {
        match kind {
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
                FsErrorKind::Io
            }
            native_files::LocalFileErrorKind::Indeterminate => {
                FsErrorKind::Indeterminate
            }
            native_files::LocalFileErrorKind::Io => FsErrorKind::Io,
            _ => FsErrorKind::Other,
        }
    }

    /// Converts standard I/O error kinds to facade error kinds.
    fn io_kind(kind: std::io::ErrorKind) -> FsErrorKind {
        match kind {
            std::io::ErrorKind::NotFound => FsErrorKind::NotFound,
            std::io::ErrorKind::AlreadyExists => FsErrorKind::AlreadyExists,
            std::io::ErrorKind::PermissionDenied => {
                FsErrorKind::PermissionDenied
            }
            std::io::ErrorKind::InvalidInput => FsErrorKind::InvalidPath,
            std::io::ErrorKind::InvalidData => FsErrorKind::DataCorruption,
            std::io::ErrorKind::Interrupted => FsErrorKind::Interrupted,
            std::io::ErrorKind::TimedOut => FsErrorKind::Timeout,
            std::io::ErrorKind::Unsupported => {
                FsErrorKind::UnsupportedOperation
            }
            std::io::ErrorKind::OutOfMemory
            | std::io::ErrorKind::StorageFull
            | std::io::ErrorKind::QuotaExceeded => {
                FsErrorKind::ResourceLimitExceeded
            }
            _ => FsErrorKind::Io,
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
            (Kind::PublicationIncomplete, FsErrorKind::Io),
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
