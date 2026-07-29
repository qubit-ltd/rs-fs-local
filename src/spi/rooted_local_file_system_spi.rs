// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Descriptor-backed rooted synchronous SPI delegated to `qubit-local-files`.

use super::error_mapper::LocalFileErrorMapper;
use super::local_directory_stream_spi::LocalDirectoryStreamSpi;
use super::local_file_writer_spi::LocalFileWriterSpi;
use super::local_options_mapper::LocalOptionsMapper;
use super::local_outcome_mapper::LocalOutcomeMapper;
use super::local_temp_resource_spi::LocalTempResourceSpi;
use crate::path::LocalPathMapper;
use qubit_fs::spi::{
    CopyAttempt,
    CopyDeclineReason,
    CopyRequest,
    CreateDirectoryRequest,
    CreateTempDirectoryRequest,
    CreateTempFileRequest,
    DeleteDirectoryRequest,
    DeleteFileRequest,
    FileSystemSpi,
    ListRequest,
    OpenReaderRequest,
    OpenWriterRequest,
    OpenedDirectoryStream,
    OpenedReader,
    OpenedTempDirectory,
    OpenedTempFile,
    OpenedWriter,
    RenameRequest,
    SpiCopyFailure,
    SpiRenameFailure,
    StatRequest,
    StatResponse,
};
use qubit_fs::{
    CreateDirectoryOutcome,
    DeleteOutcome,
    FileSystemCapabilities,
    FileSystemCapability,
    FileSystemId,
    FileSystemInfo,
    FileSystemLimits,
    FileSystemProperties,
    FsError,
    FsOperation,
    FsResult,
    OpenedFileInfo,
    PathConstraints,
    PathSemantics,
    RenameFailureState,
};
use qubit_local_files as native_files;
use std::path::Path as NativePath;

/// Opened rooted implementation of the synchronous local filesystem SPI.
pub struct RootedLocalFileSystemSpi {
    native: native_files::RootedLocalFileSystem,
    properties: FileSystemProperties,
}
impl RootedLocalFileSystemSpi {
    /// Opens `root` as a retained native authority.
    pub fn open(id: FileSystemId, root: &NativePath) -> FsResult<Self> {
        let native = native_files::RootedLocalFileSystem::open(root).map_err(
            |error| {
                FsError::with_source(
                    qubit_fs::FsErrorKind::ProviderUnavailable,
                    FsOperation::Provider,
                    "cannot open rooted local filesystem",
                    error,
                )
            },
        )?;
        let native_capabilities = native.capabilities();
        let mut capabilities = FileSystemCapabilities::new()
            .with(FileSystemCapability::List)
            .with(FileSystemCapability::Read)
            .with(FileSystemCapability::Write)
            .with(FileSystemCapability::Append)
            .with(FileSystemCapability::CreateDirectory)
            .with(FileSystemCapability::Delete)
            .with(FileSystemCapability::RecursiveDelete)
            .with(FileSystemCapability::Rename)
            .with(FileSystemCapability::Copy)
            .with(FileSystemCapability::TempFile)
            .with(FileSystemCapability::TempDirectory);
        if native_capabilities.supports_no_replace_publication() {
            capabilities = capabilities
                .with(FileSystemCapability::AtomicRename)
                .with(FileSystemCapability::AtomicReplace)
                .with(FileSystemCapability::AtomicTempPersist);
        }
        if native_capabilities.supports_directory_durability() {
            capabilities = capabilities.with(FileSystemCapability::DurableCopy);
        }
        let properties = FileSystemProperties::new(
            FileSystemInfo::new(id, "local-file", PathSemantics::Hierarchical)
                .with_scheme("file")?,
            capabilities,
            FileSystemLimits::unknown(),
            PathConstraints::absolute(),
        )?;
        Ok(Self { native, properties })
    }
    fn info(&self, path: qubit_fs::Path) -> OpenedFileInfo {
        OpenedFileInfo::new(self.properties.info().id().clone(), path)
    }
    fn map(
        &self,
        error: native_files::LocalFileError,
        operation: FsOperation,
        path: &qubit_fs::Path,
    ) -> FsError {
        LocalFileErrorMapper::map(error, operation, path, None)
    }
}
impl FileSystemSpi for RootedLocalFileSystemSpi {
    fn properties(&self) -> FileSystemProperties {
        self.properties.clone()
    }
    fn stat(&self, r: StatRequest<'_>) -> FsResult<StatResponse> {
        let path = LocalPathMapper::rooted(r.path())?;
        self.native
            .metadata(&path)
            .map(|v| {
                StatResponse::new(
                    r.path().clone(),
                    LocalOutcomeMapper::metadata(v),
                )
            })
            .map_err(|e| self.map(e, FsOperation::Stat, r.path()))
    }
    fn list(&self, r: ListRequest<'_>) -> FsResult<OpenedDirectoryStream> {
        let p = LocalPathMapper::rooted(r.path())?;
        let o = LocalOptionsMapper::list(r.options())?;
        self.native
            .list(&p, &o)
            .map(|v| {
                OpenedDirectoryStream::new(Box::new(
                    LocalDirectoryStreamSpi::rooted(
                        v,
                        r.path().clone(),
                        r.options(),
                    ),
                ))
            })
            .map_err(|e| self.map(e, FsOperation::List, r.path()))
    }
    fn open_reader(&self, r: OpenReaderRequest<'_>) -> FsResult<OpenedReader> {
        let p = LocalPathMapper::rooted(r.path())?;
        let o = LocalOptionsMapper::read(r.options());
        self.native
            .open_reader(&p, &o)
            .map(|v| {
                OpenedReader::new(self.info(r.path().clone()), Box::new(v))
            })
            .map_err(|e| self.map(e, FsOperation::OpenReader, r.path()))
    }
    fn open_writer(&self, r: OpenWriterRequest<'_>) -> FsResult<OpenedWriter> {
        let p = LocalPathMapper::rooted(r.path())?;
        let o = LocalOptionsMapper::write(r.options())?;
        self.native
            .open_writer(&p, &o)
            .map(|v| {
                OpenedWriter::new(
                    self.info(r.path().clone()),
                    Box::new(LocalFileWriterSpi::new(v)),
                )
            })
            .map_err(|e| self.map(e, FsOperation::OpenWriter, r.path()))
    }
    fn create_directory(
        &self,
        r: CreateDirectoryRequest<'_>,
    ) -> FsResult<CreateDirectoryOutcome> {
        let p = LocalPathMapper::rooted(r.path())?;
        let o = LocalOptionsMapper::create_directory(r.options())?;
        self.native
            .create_directory(&p, &o)
            .map(|v| CreateDirectoryOutcome::new(!v.created()))
            .map_err(|e| self.map(e, FsOperation::CreateDir, r.path()))
    }
    fn delete_file(&self, r: DeleteFileRequest<'_>) -> FsResult<DeleteOutcome> {
        let p = LocalPathMapper::rooted(r.path())?;
        let o = LocalOptionsMapper::delete(r.options());
        self.native
            .delete_file(&p, &o)
            .map(|v| DeleteOutcome::new(!v.deleted()))
            .map_err(|e| self.map(e, FsOperation::Delete, r.path()))
    }
    fn delete_directory(
        &self,
        r: DeleteDirectoryRequest<'_>,
    ) -> FsResult<DeleteOutcome> {
        let p = LocalPathMapper::rooted(r.path())?;
        let o = LocalOptionsMapper::delete(r.options());
        self.native
            .delete_directory(&p, &o)
            .map(|v| DeleteOutcome::new(!v.deleted()))
            .map_err(|e| self.map(e, FsOperation::Delete, r.path()))
    }
    fn try_copy(
        &self,
        request: CopyRequest<'_>,
    ) -> Result<CopyAttempt, SpiCopyFailure> {
        let options = match LocalOptionsMapper::copy(request.options()) {
            Ok(options) => options,
            Err(_) => {
                return Ok(declined_copy());
            }
        };
        let (source, target) =
            LocalPathMapper::rooted_pair(request.source(), request.target())
                .map_err(copy_path_error)?;
        self.native
            .copy(&source, &target, &options)
            .map(|value| CopyAttempt::Completed(LocalOutcomeMapper::copy(value)))
            .map_err(|error| {
                let (error, state, stats, _staging, _cleanup) = error.into_parts();
                SpiCopyFailure::new(
                    LocalFileErrorMapper::map(
                        error,
                        FsOperation::Copy,
                        request.source(),
                        Some(request.target()),
                    ),
                    match state {
                        native_files::LocalCopyFailureState::Unchanged => {
                            qubit_fs::CopyFailureState::Unchanged
                        }
                        native_files::LocalCopyFailureState::PartiallyPublished => {
                            qubit_fs::CopyFailureState::PartiallyPublished
                        }
                        native_files::LocalCopyFailureState::Published => {
                            qubit_fs::CopyFailureState::Published
                        }
                        native_files::LocalCopyFailureState::Indeterminate => {
                            qubit_fs::CopyFailureState::Indeterminate
                        }
                    },
                    qubit_fs::CopyStats {
                        files: stats.files(),
                        directories: stats.directories(),
                        bytes: stats.bytes(),
                        skipped: stats.skipped(),
                        overwritten: stats.overwritten(),
                        ..Default::default()
                    },
                )
            })
    }
    fn rename(
        &self,
        r: RenameRequest<'_>,
    ) -> Result<qubit_fs::RenameOutcome, SpiRenameFailure> {
        let (s, t) = LocalPathMapper::rooted_pair(r.source(), r.target())
            .map_err(rename_path_error)?;
        let o = LocalOptionsMapper::rename(r.options());
        self.native
            .rename(&s, &t, &o)
            .map(|value| {
                LocalOutcomeMapper::rename(value, r.source(), r.target())
            })
            .map_err(|e| {
                let (e, state) = e.into_parts();
                SpiRenameFailure::new(
                    LocalFileErrorMapper::map(
                        e,
                        FsOperation::Rename,
                        r.source(),
                        Some(r.target()),
                    ),
                    match state {
                        native_files::LocalRenameFailureState::Unchanged => {
                            RenameFailureState::Unchanged
                        }
                        native_files::LocalRenameFailureState::Renamed => {
                            RenameFailureState::Renamed
                        }
                        _ => RenameFailureState::Indeterminate,
                    },
                )
            })
    }
    fn create_temp_file(
        &self,
        request: CreateTempFileRequest,
    ) -> FsResult<OpenedTempFile> {
        let parent = request
            .options()
            .parent
            .as_ref()
            .map(LocalPathMapper::rooted)
            .transpose()?;
        let mut options = native_files::LocalTempFileOptions::new()
            .with_prefix(&request.options().prefix)
            .with_suffix(&request.options().suffix);
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        let v = self.native.create_temp_file(&options).map_err(|e| {
            FsError::with_source(
                qubit_fs::FsErrorKind::Io,
                FsOperation::CreateTemp,
                "native temporary file creation failed",
                e,
            )
        })?;
        let p = LocalPathMapper::rooted_logical(v.path())?;
        Ok(OpenedTempFile::new(
            self.info(p),
            Box::new(LocalTempResourceSpi::file(v, true)),
        ))
    }
    fn create_temp_directory(
        &self,
        request: CreateTempDirectoryRequest,
    ) -> FsResult<OpenedTempDirectory> {
        let parent = request
            .options()
            .parent
            .as_ref()
            .map(LocalPathMapper::rooted)
            .transpose()?;
        let mut options = native_files::LocalTempDirectoryOptions::new()
            .with_prefix(&request.options().prefix)
            .with_suffix(&request.options().suffix);
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        let v = self.native.create_temp_directory(&options).map_err(|e| {
            FsError::with_source(
                qubit_fs::FsErrorKind::Io,
                FsOperation::CreateTemp,
                "native temporary directory creation failed",
                e,
            )
        })?;
        let p = LocalPathMapper::rooted_logical(v.path())?;
        Ok(OpenedTempDirectory::new(
            self.info(p),
            Box::new(LocalTempResourceSpi::directory(v, true)),
        ))
    }
}

fn declined_copy() -> CopyAttempt {
    CopyAttempt::Declined(CopyDeclineReason::NotApplicable)
}

fn copy_path_error(error: FsError) -> SpiCopyFailure {
    SpiCopyFailure::new(
        error,
        qubit_fs::CopyFailureState::Unchanged,
        qubit_fs::CopyStats::default(),
    )
}

fn rename_path_error(error: FsError) -> SpiRenameFailure {
    SpiRenameFailure::new(error, RenameFailureState::Unchanged)
}

#[cfg(test)]
mod tests {
    use super::{
        copy_path_error,
        declined_copy,
        rename_path_error,
    };

    #[test]
    fn maps_rooted_preflight_copy_and_rename_outcomes() {
        assert!(matches!(
            declined_copy(),
            qubit_fs::spi::CopyAttempt::Declined(
                qubit_fs::spi::CopyDeclineReason::NotApplicable
            )
        ));
        let error = qubit_fs::FsError::new(
            qubit_fs::FsErrorKind::InvalidPath,
            qubit_fs::FsOperation::Copy,
            "test",
        );
        assert_eq!(
            qubit_fs::CopyFailureState::Unchanged,
            copy_path_error(error).state()
        );
        let error = qubit_fs::FsError::new(
            qubit_fs::FsErrorKind::InvalidPath,
            qubit_fs::FsOperation::Rename,
            "test",
        );
        assert_eq!(
            qubit_fs::RenameFailureState::Unchanged,
            rename_path_error(error).state()
        );
    }
}
