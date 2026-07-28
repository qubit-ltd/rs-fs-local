//! Host namespace synchronous SPI delegated to `qubit-local-files`.

use qubit_fs::spi::{
    CopyAttempt, CopyDeclineReason, CopyRequest, CreateDirectoryRequest,
    CreateTempDirectoryRequest, CreateTempFileRequest, DeleteDirectoryRequest, DeleteFileRequest,
    FileSystemSpi, ListRequest, OpenReaderRequest, OpenWriterRequest, OpenedDirectoryStream,
    OpenedReader, OpenedTempDirectory, OpenedTempFile, OpenedWriter, RenameRequest, SpiCopyFailure,
    SpiRenameFailure, StatRequest, StatResponse,
};
use qubit_fs::{
    CreateDirectoryOutcome, DeleteOutcome, FileSystemCapabilities, FileSystemCapability,
    FileSystemId, FileSystemInfo, FileSystemLimits, FileSystemProperties, FsError, FsErrorKind,
    FsOperation, FsResult, OpenedFileInfo, PathConstraints, PathSemantics, RenameFailureState,
};
use qubit_local_files as native_files;

use super::error_mapper::LocalFileErrorMapper;
use super::local_directory_stream_spi::LocalDirectoryStreamSpi;
use super::local_file_writer_spi::LocalFileWriterSpi;
use super::local_options_mapper::LocalOptionsMapper;
use super::local_outcome_mapper::LocalOutcomeMapper;
use super::local_temp_resource_spi::LocalTempResourceSpi;
use crate::path::LocalPathMapper;

/// Host-wide implementation of the synchronous local filesystem SPI.
pub struct LocalFileSystemSpi {
    properties: FileSystemProperties,
}
impl LocalFileSystemSpi {
    /// Creates the fixed host filesystem implementation.
    pub fn new() -> Self {
        Self {
            properties: Self::properties_snapshot(),
        }
    }
    fn properties_snapshot() -> FileSystemProperties {
        FileSystemProperties::new(
            FileSystemInfo::new(
                FileSystemId::new("local-host").expect("static filesystem identity is valid"),
                "file",
                PathSemantics::Hierarchical,
            )
            .with_scheme("file")
            .expect("static URI scheme is valid"),
            FileSystemCapabilities::new()
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
                .with(FileSystemCapability::TempDirectory)
                .with(FileSystemCapability::AtomicReplace)
                .with(FileSystemCapability::AtomicTempPersist),
            FileSystemLimits::unknown(),
            PathConstraints::absolute(),
        )
        .expect("static properties are valid")
    }
    fn info(&self, path: qubit_fs::Path) -> OpenedFileInfo {
        OpenedFileInfo::new(self.properties.info().id().clone(), path)
    }
    fn map(
        error: native_files::LocalFileError,
        operation: FsOperation,
        path: &qubit_fs::Path,
    ) -> FsError {
        LocalFileErrorMapper::map(error, operation, path, None)
    }
}
impl Default for LocalFileSystemSpi {
    fn default() -> Self {
        Self::new()
    }
}
impl FileSystemSpi for LocalFileSystemSpi {
    fn properties(&self) -> FileSystemProperties {
        self.properties.clone()
    }
    fn stat(&self, request: StatRequest<'_>) -> FsResult<StatResponse> {
        let path = LocalPathMapper::host(request.path())?;
        native_files::LocalFileSystem::metadata(&path)
            .map(|value| {
                StatResponse::new(request.path().clone(), LocalOutcomeMapper::metadata(value))
            })
            .map_err(|error| Self::map(error, FsOperation::Stat, request.path()))
    }
    fn list(&self, request: ListRequest<'_>) -> FsResult<OpenedDirectoryStream> {
        let path = LocalPathMapper::host(request.path())?;
        let options = LocalOptionsMapper::list(request.options());
        native_files::LocalFileSystem::list(&path, &options)
            .map(|value| OpenedDirectoryStream::new(Box::new(LocalDirectoryStreamSpi::host(value))))
            .map_err(|error| Self::map(error, FsOperation::List, request.path()))
    }
    fn open_reader(&self, request: OpenReaderRequest<'_>) -> FsResult<OpenedReader> {
        let path = LocalPathMapper::host(request.path())?;
        let options = LocalOptionsMapper::read(request.options());
        native_files::LocalFileSystem::open_reader(&path, &options)
            .map(|value| OpenedReader::new(self.info(request.path().clone()), Box::new(value)))
            .map_err(|error| Self::map(error, FsOperation::OpenReader, request.path()))
    }
    fn open_writer(&self, request: OpenWriterRequest<'_>) -> FsResult<OpenedWriter> {
        let path = LocalPathMapper::host(request.path())?;
        let options = LocalOptionsMapper::write(request.options())?;
        native_files::LocalFileSystem::open_writer(&path, &options)
            .map(|value| {
                OpenedWriter::new(
                    self.info(request.path().clone()),
                    Box::new(LocalFileWriterSpi::new(value)),
                )
            })
            .map_err(|error| Self::map(error, FsOperation::OpenWriter, request.path()))
    }
    fn create_directory(
        &self,
        request: CreateDirectoryRequest<'_>,
    ) -> FsResult<CreateDirectoryOutcome> {
        let path = LocalPathMapper::host(request.path())?;
        let options = LocalOptionsMapper::create_directory(request.options());
        native_files::LocalFileSystem::create_directory(&path, &options)
            .map(|value| CreateDirectoryOutcome::new(!value.created()))
            .map_err(|error| Self::map(error, FsOperation::CreateDir, request.path()))
    }
    fn delete_file(&self, request: DeleteFileRequest<'_>) -> FsResult<DeleteOutcome> {
        let path = LocalPathMapper::host(request.path())?;
        let options = LocalOptionsMapper::delete(request.options());
        native_files::LocalFileSystem::delete_file(&path, &options)
            .map(|value| DeleteOutcome::new(!value.deleted()))
            .map_err(|error| Self::map(error, FsOperation::Delete, request.path()))
    }
    fn delete_directory(&self, request: DeleteDirectoryRequest<'_>) -> FsResult<DeleteOutcome> {
        let path = LocalPathMapper::host(request.path())?;
        let options = LocalOptionsMapper::delete(request.options());
        native_files::LocalFileSystem::delete_directory(&path, &options)
            .map(|value| DeleteOutcome::new(!value.deleted()))
            .map_err(|error| Self::map(error, FsOperation::Delete, request.path()))
    }
    fn try_copy(&self, request: CopyRequest<'_>) -> Result<CopyAttempt, SpiCopyFailure> {
        let options = match LocalOptionsMapper::copy(request.options()) {
            Ok(options) => options,
            Err(_) => return Ok(CopyAttempt::Declined(CopyDeclineReason::NotApplicable)),
        };
        let (source, target) = LocalPathMapper::host_pair(request.source(), request.target())
            .map_err(|error| {
                SpiCopyFailure::new(
                    error,
                    qubit_fs::CopyFailureState::Unchanged,
                    qubit_fs::CopyStats::default(),
                )
            })?;
        native_files::LocalFileSystem::copy(&source, &target, &options)
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
        request: RenameRequest<'_>,
    ) -> Result<qubit_fs::RenameOutcome, SpiRenameFailure> {
        let (source, target) = LocalPathMapper::host_pair(request.source(), request.target())
            .map_err(|error| SpiRenameFailure::new(error, RenameFailureState::Unchanged))?;
        let options = LocalOptionsMapper::rename(request.options());
        native_files::LocalFileSystem::rename(&source, &target, &options)
            .map(LocalOutcomeMapper::rename)
            .map_err(|error| {
                let (error, state) = error.into_parts();
                SpiRenameFailure::new(
                    LocalFileErrorMapper::map(
                        error,
                        FsOperation::Rename,
                        request.source(),
                        Some(request.target()),
                    ),
                    match state {
                        native_files::LocalRenameFailureState::Unchanged => {
                            RenameFailureState::Unchanged
                        }
                        native_files::LocalRenameFailureState::Renamed => {
                            RenameFailureState::Renamed
                        }
                        native_files::LocalRenameFailureState::Indeterminate => {
                            RenameFailureState::Indeterminate
                        }
                    },
                )
            })
    }
    fn create_temp_file(&self, request: CreateTempFileRequest) -> FsResult<OpenedTempFile> {
        let parent = request
            .options()
            .parent
            .as_ref()
            .map(LocalPathMapper::host)
            .transpose()?;
        let mut options = native_files::LocalTempFileOptions::new()
            .with_prefix(&request.options().prefix)
            .with_suffix(&request.options().suffix);
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        let value = native_files::LocalFileSystem::create_temp_file(&options).map_err(|error| {
            FsError::with_source(
                FsErrorKind::Io,
                FsOperation::CreateTemp,
                "native temporary file creation failed",
                error,
            )
        })?;
        let path = LocalPathMapper::host_logical(value.path())?;
        Ok(OpenedTempFile::new(
            self.info(path),
            Box::new(LocalTempResourceSpi::file(value, false)),
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
            .map(LocalPathMapper::host)
            .transpose()?;
        let mut options = native_files::LocalTempDirectoryOptions::new()
            .with_prefix(&request.options().prefix)
            .with_suffix(&request.options().suffix);
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        let value =
            native_files::LocalFileSystem::create_temp_directory(&options).map_err(|error| {
                FsError::with_source(
                    FsErrorKind::Io,
                    FsOperation::CreateTemp,
                    "native temporary directory creation failed",
                    error,
                )
            })?;
        let path = LocalPathMapper::host_logical(value.path())?;
        Ok(OpenedTempDirectory::new(
            self.info(path),
            Box::new(LocalTempResourceSpi::directory(value, false)),
        ))
    }
}
