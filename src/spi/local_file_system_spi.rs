// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Host namespace synchronous SPI delegated to `qubit-local-files`.

use qubit_fs::spi::{
    CopyAttempt,
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
    CopyFailureState,
    CopyStats,
    CreateDirectoryOutcome,
    DeleteOutcome,
    FileKind,
    FileMetadata,
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
};
use qubit_local_files as native_files;

use super::error_mapper;
use super::local_directory_stream_spi::LocalDirectoryStreamSpi;
use super::local_file_writer_spi::LocalFileWriterSpi;
use super::local_options_mapper;
use super::local_outcome_mapper;
use super::local_temp_resource_spi::LocalTempResourceSpi;
use crate::constants::{
    FILE_SCHEME,
    LOCAL_PROVIDER_ID,
};
use crate::path::local_path_mapper;

/// Host-wide implementation of the synchronous local filesystem SPI.
#[must_use]
pub struct LocalFileSystemSpi {
    /// Immutable capabilities, limits, path rules, and provider identity.
    properties: FileSystemProperties,
    /// Provider identity attached to every translated failure.
    provider_id: String,
}

impl LocalFileSystemSpi {
    /// Creates the fixed host filesystem implementation.
    ///
    /// # Returns
    ///
    /// A host SPI with capabilities derived from the native backend.
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            properties: Self::properties_snapshot(),
            provider_id: LOCAL_PROVIDER_ID.to_owned(),
        }
    }

    /// Builds the immutable host filesystem property snapshot.
    ///
    /// # Returns
    ///
    /// Properties for the `local-host` identity and current native
    /// capabilities.
    ///
    /// # Panics
    ///
    /// Panics only if the static filesystem identity, `file` scheme, or
    /// internally assembled property set violates a `qubit-fs` invariant.
    fn properties_snapshot() -> FileSystemProperties {
        let native_capabilities = native_files::LocalFileSystem::capabilities();
        let mut capabilities = FileSystemCapabilities::new()
            .with(FileSystemCapability::List)
            .with(FileSystemCapability::Read)
            .with(FileSystemCapability::Write)
            .with(FileSystemCapability::Append)
            .with(FileSystemCapability::CreateDirectory)
            .with(FileSystemCapability::EmptyDirectory)
            .with(FileSystemCapability::Delete)
            .with(FileSystemCapability::RecursiveDelete)
            .with(FileSystemCapability::Rename)
            .with(FileSystemCapability::Copy)
            .with(FileSystemCapability::TempFile)
            .with(FileSystemCapability::TempDirectory);
        if native_capabilities.atomic_rename_implemented() {
            capabilities =
                capabilities.with(FileSystemCapability::AtomicRename);
        }
        if native_capabilities.atomic_replace_implemented() {
            capabilities =
                capabilities.with(FileSystemCapability::AtomicReplace);
        }
        if native_capabilities.atomic_temp_persist_implemented() {
            capabilities =
                capabilities.with(FileSystemCapability::AtomicTempPersist);
        }
        if native_capabilities.directory_durability_implemented() {
            capabilities = capabilities.with(FileSystemCapability::DurableCopy);
        }
        FileSystemProperties::new(
            FileSystemInfo::new(
                FileSystemId::new("local-host")
                    .expect("static filesystem identity is valid"),
                LOCAL_PROVIDER_ID,
                PathSemantics::Hierarchical,
            )
            .with_scheme(FILE_SCHEME)
            .expect("static URI scheme is valid"),
            capabilities,
            FileSystemLimits::unknown(),
            PathConstraints::absolute(),
        )
        .expect("static properties are valid")
    }

    /// Creates opened-file context for one logical path.
    ///
    /// # Parameters
    ///
    /// - `path`: Logical path associated with the opened resource.
    ///
    /// # Returns
    ///
    /// Opened-file information containing the host filesystem identity.
    #[inline(always)]
    fn info(&self, path: qubit_fs::Path) -> OpenedFileInfo {
        OpenedFileInfo::new(self.properties.info().id().clone(), path)
    }

    /// Maps a native failure with one logical request path.
    ///
    /// # Parameters
    ///
    /// - `error`: Native local-files failure.
    /// - `operation`: Facade operation that failed.
    /// - `path`: Logical request path.
    ///
    /// # Returns
    ///
    /// A facade error with translated kind and local provider context.
    #[inline(always)]
    fn map(
        &self,
        error: native_files::LocalFileError,
        operation: FsOperation,
        path: &qubit_fs::Path,
    ) -> FsError {
        error_mapper::map(error, operation, path, None, &self.provider_id)
    }
}

impl Default for LocalFileSystemSpi {
    /// Creates the fixed host filesystem implementation.
    ///
    /// # Returns
    ///
    /// The same host SPI produced by [`Self::new`].
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystemSpi for LocalFileSystemSpi {
    /// Returns the immutable host filesystem properties.
    ///
    /// # Returns
    ///
    /// A snapshot of host identity, capabilities, limits, and path rules.
    #[inline(always)]
    fn properties(&self) -> FileSystemProperties {
        self.properties.clone()
    }

    /// Reads metadata for a host logical path.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved stat request.
    ///
    /// # Returns
    ///
    /// Portable metadata associated with the requested logical path.
    ///
    /// # Errors
    ///
    /// Returns path-conversion errors or mapped native metadata failures.
    fn stat(&self, request: StatRequest<'_>) -> FsResult<StatResponse> {
        let path = local_path_mapper::host(request.path())?;
        native_files::LocalFileSystem::metadata(&path)
            .map(|value| {
                StatResponse::new(
                    request.path().clone(),
                    local_outcome_mapper::metadata(value),
                )
            })
            .map_err(|error| self.map(error, FsOperation::Stat, request.path()))
    }

    /// Opens a lazy host directory listing.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved list request and filtering options.
    ///
    /// # Returns
    ///
    /// A directory stream that maps native entries to logical paths.
    ///
    /// # Errors
    ///
    /// Returns path or option conversion errors and mapped native list
    /// failures.
    fn list(
        &self,
        request: ListRequest<'_>,
    ) -> FsResult<OpenedDirectoryStream> {
        let path = local_path_mapper::host(request.path())?;
        let options = local_options_mapper::list(request.options())?;
        native_files::LocalFileSystem::list(&path, &options)
            .map(|value| {
                OpenedDirectoryStream::new(Box::new(
                    LocalDirectoryStreamSpi::host(
                        value,
                        request.options(),
                        &self.provider_id,
                    ),
                ))
            })
            .map_err(|error| self.map(error, FsOperation::List, request.path()))
    }

    /// Opens a host file for reading.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved reader request.
    ///
    /// # Returns
    ///
    /// An opened reader retaining logical file information.
    ///
    /// # Errors
    ///
    /// Returns path-conversion errors or mapped native open failures.
    fn open_reader(
        &self,
        request: OpenReaderRequest<'_>,
    ) -> FsResult<OpenedReader> {
        let path = local_path_mapper::host(request.path())?;
        let options = local_options_mapper::read(request.options());
        native_files::LocalFileSystem::open_reader(&path, &options)
            .map(|value| {
                OpenedReader::new(
                    self.info(request.path().clone()),
                    Box::new(value),
                )
            })
            .map_err(|error| {
                self.map(error, FsOperation::OpenReader, request.path())
            })
    }

    /// Opens a host file for stateful publication.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved writer request and publication requirements.
    ///
    /// # Returns
    ///
    /// An opened writer with explicit commit and abort lifecycle.
    ///
    /// # Errors
    ///
    /// Returns path or option conversion errors and mapped native open
    /// failures.
    fn open_writer(
        &self,
        request: OpenWriterRequest<'_>,
    ) -> FsResult<OpenedWriter> {
        let path = local_path_mapper::host(request.path())?;
        let options = local_options_mapper::write(request.options())?;
        native_files::LocalFileSystem::open_writer(&path, &options)
            .map(|value| {
                OpenedWriter::new(
                    self.info(request.path().clone()),
                    Box::new(LocalFileWriterSpi::new(
                        value,
                        self.provider_id.clone(),
                    )),
                )
            })
            .map_err(|error| {
                self.map(error, FsOperation::OpenWriter, request.path())
            })
    }

    /// Creates a host directory using resolved facade policy.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved directory-creation request.
    ///
    /// # Returns
    ///
    /// An outcome reporting whether the directory already existed.
    ///
    /// # Errors
    ///
    /// Returns path or option conversion errors and mapped native creation
    /// failures.
    fn create_directory(
        &self,
        request: CreateDirectoryRequest<'_>,
    ) -> FsResult<CreateDirectoryOutcome> {
        let path = local_path_mapper::host(request.path())?;
        let options =
            local_options_mapper::create_directory(request.options())?;
        native_files::LocalFileSystem::create_directory(&path, &options)
            .map(|value| CreateDirectoryOutcome::new(!value.created()))
            .map_err(|error| {
                self.map(error, FsOperation::CreateDir, request.path())
            })
    }

    /// Deletes one host file.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved file-deletion request.
    ///
    /// # Returns
    ///
    /// An outcome reporting whether the entry was already absent.
    ///
    /// # Errors
    ///
    /// Returns path-conversion errors or mapped native deletion failures.
    fn delete_file(
        &self,
        request: DeleteFileRequest<'_>,
    ) -> FsResult<DeleteOutcome> {
        let path = local_path_mapper::host(request.path())?;
        let options = local_options_mapper::delete(request.options());
        native_files::LocalFileSystem::delete_file(&path, &options)
            .map(|value| DeleteOutcome::new(!value.deleted()))
            .map_err(|error| {
                self.map(error, FsOperation::Delete, request.path())
            })
    }

    /// Deletes one host directory.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved directory-deletion request.
    ///
    /// # Returns
    ///
    /// An outcome reporting whether the entry was already absent.
    ///
    /// # Errors
    ///
    /// Returns path-conversion errors or mapped native deletion failures.
    fn delete_directory(
        &self,
        request: DeleteDirectoryRequest<'_>,
    ) -> FsResult<DeleteOutcome> {
        let path = local_path_mapper::host(request.path())?;
        let options = local_options_mapper::delete(request.options());
        native_files::LocalFileSystem::delete_directory(&path, &options)
            .map(|value| DeleteOutcome::new(!value.deleted()))
            .map_err(|error| {
                self.map(error, FsOperation::Delete, request.path())
            })
    }

    /// Attempts a native host copy when all requirements are expressible.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved source, target, and copy policy.
    ///
    /// # Returns
    ///
    /// `Completed` for a native copy or `Declined` when its options require
    /// facade fallback.
    ///
    /// # Errors
    ///
    /// Returns a structured copy failure for path conversion or native copy
    /// errors, preserving publication state and partial statistics.
    fn try_copy(
        &self,
        request: CopyRequest<'_>,
    ) -> Result<CopyAttempt, SpiCopyFailure> {
        let options = match local_options_mapper::copy(request.options()) {
            Ok(options) => options,
            Err(error) => {
                return Err(SpiCopyFailure::new(
                    error
                        .with_operation(FsOperation::Copy)
                        .with_path(request.source().clone())
                        .with_target(request.target().clone())
                        .with_provider(&self.provider_id),
                    CopyFailureState::Unchanged,
                    CopyStats::default(),
                ));
            }
        };
        let (source, target) =
            local_path_mapper::host_pair(request.source(), request.target())
                .map_err(error_mapper::copy_path_error)?;
        native_files::LocalFileSystem::copy(&source, &target, &options)
            .map(|value| {
                CopyAttempt::Completed(local_outcome_mapper::copy(value))
            })
            .map_err(|error| {
                let state = error.state();
                let stats = *error.partial_stats();
                SpiCopyFailure::new(
                    error_mapper::copy_failure(
                        error,
                        request.source(),
                        request.target(),
                        &self.provider_id,
                    ),
                    local_outcome_mapper::copy_failure_state(state),
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

    /// Renames one host path to another through native atomic rename.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved source, target, and rename policy.
    ///
    /// # Returns
    ///
    /// A completed portable rename outcome.
    ///
    /// # Errors
    ///
    /// Returns a structured failure for path conversion or native rename
    /// errors, preserving the known namespace state.
    fn rename(
        &self,
        request: RenameRequest<'_>,
    ) -> Result<qubit_fs::RenameOutcome, SpiRenameFailure> {
        let (source, target) =
            local_path_mapper::host_pair(request.source(), request.target())
                .map_err(error_mapper::rename_path_error)?;
        let options = local_options_mapper::rename(request.options());
        native_files::LocalFileSystem::rename(&source, &target, &options)
            .map(|value| {
                local_outcome_mapper::rename(
                    value,
                    request.source(),
                    request.target(),
                )
            })
            .map_err(|error| {
                let (error, state) = error.into_parts();
                SpiRenameFailure::new(
                    error_mapper::map(
                        error,
                        FsOperation::Rename,
                        request.source(),
                        Some(request.target()),
                        &self.provider_id,
                    ),
                    local_outcome_mapper::rename_failure_state(state),
                )
            })
    }

    /// Creates a temporary file in the host authority.
    ///
    /// # Parameters
    ///
    /// - `request`: Parent, prefix, and suffix options.
    ///
    /// # Returns
    ///
    /// An opened temporary file with host-mode lifecycle ownership.
    ///
    /// # Errors
    ///
    /// Returns path-conversion errors or mapped native creation failures.
    fn create_temp_file(
        &self,
        request: CreateTempFileRequest,
    ) -> FsResult<OpenedTempFile> {
        let parent = request
            .options()
            .parent
            .as_ref()
            .map(local_path_mapper::host)
            .transpose()?;
        let mut options = native_files::LocalTempFileOptions::new()
            .with_prefix(&request.options().prefix)
            .with_suffix(&request.options().suffix);
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        let value = native_files::LocalFileSystem::create_temp_file(&options)
            .map_err(|error| {
            error_mapper::map_without_path(
                error,
                FsOperation::CreateTemp,
                "native temporary file creation failed",
                &self.provider_id,
            )
        })?;
        let path = local_path_mapper::host_logical(
            value.path(),
            FsOperation::CreateTemp,
        )?;
        Ok(OpenedTempFile::new(
            self.info(path)
                .with_metadata(FileMetadata::new(FileKind::File)),
            Box::new(LocalTempResourceSpi::file(
                value,
                false,
                self.provider_id.clone(),
            )),
        ))
    }

    /// Creates a temporary directory in the host authority.
    ///
    /// # Parameters
    ///
    /// - `request`: Parent, prefix, and suffix options.
    ///
    /// # Returns
    ///
    /// An opened temporary directory with host-mode lifecycle ownership.
    ///
    /// # Errors
    ///
    /// Returns path-conversion errors or mapped native creation failures.
    fn create_temp_directory(
        &self,
        request: CreateTempDirectoryRequest,
    ) -> FsResult<OpenedTempDirectory> {
        let parent = request
            .options()
            .parent
            .as_ref()
            .map(local_path_mapper::host)
            .transpose()?;
        let mut options = native_files::LocalTempDirectoryOptions::new()
            .with_prefix(&request.options().prefix)
            .with_suffix(&request.options().suffix);
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        let value =
            native_files::LocalFileSystem::create_temp_directory(&options)
                .map_err(|error| {
                    error_mapper::map_without_path(
                        error,
                        FsOperation::CreateTemp,
                        "native temporary directory creation failed",
                        &self.provider_id,
                    )
                })?;
        let path = local_path_mapper::host_logical(
            value.path(),
            FsOperation::CreateTemp,
        )?;
        Ok(OpenedTempDirectory::new(
            self.info(path)
                .with_metadata(FileMetadata::new(FileKind::Directory)),
            Box::new(LocalTempResourceSpi::directory(
                value,
                false,
                self.provider_id.clone(),
            )),
        ))
    }
}
