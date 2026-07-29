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
        .with_provider("file");
        match target {
            Some(target) => error.with_target(target.clone()),
            None => error,
        }
    }
}
