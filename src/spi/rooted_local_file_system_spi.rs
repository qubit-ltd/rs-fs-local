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

use std::path::Path as NativePath;

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

/// Opened rooted implementation of the synchronous local filesystem SPI.
#[must_use]
pub struct RootedLocalFileSystemSpi {
    /// Native filesystem retaining descriptor-backed root authority.
    native: native_files::RootedLocalFileSystem,
    /// Immutable identity, capabilities, limits, and path constraints.
    properties: FileSystemProperties,
    /// Provider identity attached to every translated failure.
    provider_id: String,
}

impl RootedLocalFileSystemSpi {
    /// Opens `root` as a retained native authority.
    ///
    /// # Parameters
    ///
    /// - `id`: Stable identity exposed by the opened filesystem.
    /// - `root`: Native directory retained as filesystem authority.
    ///
    /// # Returns
    ///
    /// The opened rooted SPI and its derived capability snapshot.
    ///
    /// # Errors
    ///
    /// Returns `ProviderUnavailable` when native root opening fails, or a
    /// property-construction error when the supplied identity or static scheme
    /// violates filesystem invariants.
    pub fn open(id: FileSystemId, root: &NativePath) -> FsResult<Self> {
        Self::open_with_provider_id(id, LOCAL_PROVIDER_ID, root)
    }

    /// Opens `root` with an explicit provider identity.
    ///
    /// The provider identity is kept separate from the configured filesystem
    /// identity so multiple rooted providers can coexist in one registry.
    pub fn open_with_provider_id(
        id: FileSystemId,
        provider_id: impl std::fmt::Display,
        root: &NativePath,
    ) -> FsResult<Self> {
        let provider_id = provider_id.to_string();
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
        let properties = FileSystemProperties::new(
            FileSystemInfo::new(
                id,
                provider_id.clone(),
                PathSemantics::Hierarchical,
            )
            .with_scheme(FILE_SCHEME)?,
            capabilities,
            FileSystemLimits::unknown(),
            PathConstraints::absolute(),
        )?;
        Ok(Self {
            native,
            properties,
            provider_id,
        })
    }

    /// Creates opened-file context for one logical rooted path.
    ///
    /// # Parameters
    ///
    /// - `path`: Logical path associated with the opened resource.
    ///
    /// # Returns
    ///
    /// Opened-file information containing this rooted filesystem identity.
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

impl FileSystemSpi for RootedLocalFileSystemSpi {
    /// Returns the immutable rooted filesystem properties.
    ///
    /// # Returns
    ///
    /// A snapshot of rooted identity, capabilities, limits, and path rules.
    #[inline(always)]
    fn properties(&self) -> FileSystemProperties {
        self.properties.clone()
    }

    /// Reads metadata below the retained root authority.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved stat request.
    ///
    /// # Returns
    ///
    /// Portable metadata associated with the logical request path.
    ///
    /// # Errors
    ///
    /// Returns path-conversion errors or mapped native metadata failures.
    fn stat(&self, request: StatRequest<'_>) -> FsResult<StatResponse> {
        let path = local_path_mapper::rooted(request.path())?;
        self.native
            .metadata(&path)
            .map(|value| {
                StatResponse::new(
                    request.path().clone(),
                    local_outcome_mapper::metadata(value),
                )
            })
            .map_err(|error| self.map(error, FsOperation::Stat, request.path()))
    }

    /// Opens a lazy directory listing below the retained root.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved list request and filtering options.
    ///
    /// # Returns
    ///
    /// A stream mapping native relative entries below the logical request path.
    ///
    /// # Errors
    ///
    /// Returns path or option conversion errors and mapped native list
    /// failures.
    fn list(
        &self,
        request: ListRequest<'_>,
    ) -> FsResult<OpenedDirectoryStream> {
        let path = local_path_mapper::rooted(request.path())?;
        let options = local_options_mapper::list(request.options())?;
        self.native
            .list(&path, &options)
            .map(|value| {
                OpenedDirectoryStream::new(Box::new(
                    LocalDirectoryStreamSpi::rooted(
                        value,
                        request.path().clone(),
                        request.options(),
                        &self.provider_id,
                    ),
                ))
            })
            .map_err(|error| self.map(error, FsOperation::List, request.path()))
    }

    /// Opens a rooted file for reading.
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
        let path = local_path_mapper::rooted(request.path())?;
        let options = local_options_mapper::read(request.options());
        self.native
            .open_reader(&path, &options)
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

    /// Opens a rooted file for stateful publication.
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
        let path = local_path_mapper::rooted(request.path())?;
        let options = local_options_mapper::write(request.options())?;
        self.native
            .open_writer(&path, &options)
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

    /// Creates a directory below the retained root.
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
        let path = local_path_mapper::rooted(request.path())?;
        let options =
            local_options_mapper::create_directory(request.options())?;
        self.native
            .create_directory(&path, &options)
            .map(|value| CreateDirectoryOutcome::new(!value.created()))
            .map_err(|error| {
                self.map(error, FsOperation::CreateDir, request.path())
            })
    }

    /// Deletes one file below the retained root.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved file-deletion request.
    ///
    /// # Returns
    ///
    /// An outcome reporting whether the file was already absent.
    ///
    /// # Errors
    ///
    /// Returns path-conversion errors or mapped native deletion failures.
    fn delete_file(
        &self,
        request: DeleteFileRequest<'_>,
    ) -> FsResult<DeleteOutcome> {
        let path = local_path_mapper::rooted(request.path())?;
        let options = local_options_mapper::delete(request.options());
        self.native
            .delete_file(&path, &options)
            .map(|value| DeleteOutcome::new(!value.deleted()))
            .map_err(|error| {
                self.map(error, FsOperation::Delete, request.path())
            })
    }

    /// Deletes one directory below the retained root.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved directory-deletion request.
    ///
    /// # Returns
    ///
    /// An outcome reporting whether the directory was already absent.
    ///
    /// # Errors
    ///
    /// Returns path-conversion errors or mapped native deletion failures.
    fn delete_directory(
        &self,
        request: DeleteDirectoryRequest<'_>,
    ) -> FsResult<DeleteOutcome> {
        let path = local_path_mapper::rooted(request.path())?;
        let options = local_options_mapper::delete(request.options());
        self.native
            .delete_directory(&path, &options)
            .map(|value| DeleteOutcome::new(!value.deleted()))
            .map_err(|error| {
                self.map(error, FsOperation::Delete, request.path())
            })
    }

    /// Attempts a native rooted copy when all requirements are expressible.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved source, target, and copy policy.
    ///
    /// # Returns
    ///
    /// `Completed` for a native copy or `Declined` when facade fallback is
    /// required.
    ///
    /// # Errors
    ///
    /// Returns a structured failure for path conversion or native copy errors,
    /// preserving publication state and partial statistics.
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
            local_path_mapper::rooted_pair(request.source(), request.target())
                .map_err(error_mapper::copy_path_error)?;
        self.native
            .copy(&source, &target, &options)
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

    /// Renames one rooted path to another through the retained authority.
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
            local_path_mapper::rooted_pair(request.source(), request.target())
                .map_err(error_mapper::rename_path_error)?;
        let options = local_options_mapper::rename(request.options());
        self.native
            .rename(&source, &target, &options)
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

    /// Creates a temporary file below the retained root.
    ///
    /// # Parameters
    ///
    /// - `request`: Parent, prefix, and suffix options.
    ///
    /// # Returns
    ///
    /// An opened temporary file retaining rooted lifecycle authority.
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
            .map(local_path_mapper::rooted)
            .transpose()?;
        let mut options = native_files::LocalTempFileOptions::new()
            .with_prefix(&request.options().prefix)
            .with_suffix(&request.options().suffix);
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        let value =
            self.native.create_temp_file(&options).map_err(|error| {
                error_mapper::map_without_path(
                    error,
                    FsOperation::CreateTemp,
                    "native temporary file creation failed",
                    &self.provider_id,
                )
            })?;
        let path = local_path_mapper::rooted_logical(
            value.path(),
            FsOperation::CreateTemp,
        )?;
        Ok(OpenedTempFile::new(
            self.info(path)
                .with_metadata(FileMetadata::new(FileKind::File)),
            Box::new(LocalTempResourceSpi::file(
                value,
                true,
                self.provider_id.clone(),
            )),
        ))
    }

    /// Creates a temporary directory below the retained root.
    ///
    /// # Parameters
    ///
    /// - `request`: Parent, prefix, and suffix options.
    ///
    /// # Returns
    ///
    /// An opened temporary directory retaining rooted lifecycle authority.
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
            .map(local_path_mapper::rooted)
            .transpose()?;
        let mut options = native_files::LocalTempDirectoryOptions::new()
            .with_prefix(&request.options().prefix)
            .with_suffix(&request.options().suffix);
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        let value =
            self.native
                .create_temp_directory(&options)
                .map_err(|error| {
                    error_mapper::map_without_path(
                        error,
                        FsOperation::CreateTemp,
                        "native temporary directory creation failed",
                        &self.provider_id,
                    )
                })?;
        let path = local_path_mapper::rooted_logical(
            value.path(),
            FsOperation::CreateTemp,
        )?;
        Ok(OpenedTempDirectory::new(
            self.info(path)
                .with_metadata(FileMetadata::new(FileKind::Directory)),
            Box::new(LocalTempResourceSpi::directory(
                value,
                true,
                self.provider_id.clone(),
            )),
        ))
    }
}
