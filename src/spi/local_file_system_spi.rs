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

use std::path::Path as NativePath;
use std::path::PathBuf;

use qubit_fs::CopyFailureState;
use qubit_fs::CopyStats;
use qubit_fs::CreateDirectoryOutcome;
use qubit_fs::DeleteOutcome;
use qubit_fs::FileKind;
use qubit_fs::FileMetadata;
use qubit_fs::FileSystemCapabilities;
use qubit_fs::FileSystemCapability;
use qubit_fs::FileSystemId;
use qubit_fs::FileSystemInfo;
use qubit_fs::FileSystemLimit;
use qubit_fs::FileSystemLimits;
use qubit_fs::FileSystemProperties;
use qubit_fs::FsError;
use qubit_fs::FsErrorKind;
use qubit_fs::FsOperation;
use qubit_fs::FsResult;
use qubit_fs::OpenedFileInfo;
use qubit_fs::Path;
use qubit_fs::PathConstraints;
use qubit_fs::PathSemantics;
use qubit_fs::RenameOutcome;
use qubit_fs::SymlinkPolicy;
use qubit_fs::spi::CopyAttempt;
use qubit_fs::spi::CopyRequest;
use qubit_fs::spi::CreateDirectoryRequest;
use qubit_fs::spi::CreateTempDirectoryRequest;
use qubit_fs::spi::CreateTempFileRequest;
use qubit_fs::spi::DeleteDirectoryRequest;
use qubit_fs::spi::DeleteFileRequest;
use qubit_fs::spi::FileSystemSpi;
use qubit_fs::spi::ListRequest;
use qubit_fs::spi::OpenReaderRequest;
use qubit_fs::spi::OpenWriterRequest;
use qubit_fs::spi::OpenedDirectoryStream;
use qubit_fs::spi::OpenedReader;
use qubit_fs::spi::OpenedTempDirectory;
use qubit_fs::spi::OpenedTempFile;
use qubit_fs::spi::OpenedWriter;
use qubit_fs::spi::RenameRequest;
use qubit_fs::spi::SpiCopyFailure;
use qubit_fs::spi::SpiRenameFailure;
use qubit_fs::spi::StatRequest;
use qubit_fs::spi::StatResponse;
use qubit_local_files as native_files;

use super::error_mapper;
use super::local_directory_stream_spi::LocalDirectoryStreamSpi;
use super::local_file_writer_spi::LocalFileWriterSpi;
use super::local_options_mapper;
use super::local_outcome_mapper;
use super::local_temp_resource_spi::LocalTempResourceSpi;
use crate::constants::FILE_SCHEME;
use crate::constants::LOCAL_PROVIDER_ID;
use crate::path::local_path_mapper;

/// Host-wide implementation of the synchronous local filesystem SPI.
#[must_use]
pub struct LocalFileSystemSpi {
    /// Configured Host or Rooted native filesystem engine.
    native: native_files::LocalFileSystem,
    /// Immutable capability support, limits, path rules, and provider
    /// identity.
    properties: FileSystemProperties,
    /// Provider identity attached to every translated failure.
    provider_id: String,
}

impl LocalFileSystemSpi {
    /// Creates the fixed host filesystem implementation.
    ///
    /// # Returns
    ///
    /// A host SPI with capability support derived from the native backend.
    #[inline(always)]
    pub fn new() -> Self {
        let native = native_files::LocalFileSystem::host();
        Self {
            properties: Self::properties_snapshot(
                FileSystemId::new("local-host")
                    .expect("static filesystem identity is valid"),
                LOCAL_PROVIDER_ID,
                &native,
            )
            .expect("static properties are valid"),
            provider_id: LOCAL_PROVIDER_ID.to_owned(),
            native,
        }
    }

    /// Opens a Rooted filesystem with the default local provider identity.
    ///
    /// # Errors
    ///
    /// Returns a provider error when the native authority or portable
    /// properties cannot be constructed.
    pub fn rooted(id: FileSystemId, root: &NativePath) -> FsResult<Self> {
        Self::rooted_with_provider_id(id, LOCAL_PROVIDER_ID, root)
    }

    /// Opens a Rooted filesystem with an explicit provider identity.
    ///
    /// # Errors
    ///
    /// Returns a provider error when the native authority or portable
    /// properties cannot be constructed.
    pub(crate) fn rooted_with_provider_id(
        id: FileSystemId,
        provider_id: &str,
        root: &NativePath,
    ) -> FsResult<Self> {
        let native =
            native_files::LocalFileSystem::rooted(root).map_err(|error| {
                FsError::with_source(
                    FsErrorKind::ProviderUnavailable,
                    FsOperation::Provider,
                    "cannot open rooted local filesystem",
                    error,
                )
            })?;
        let properties = Self::properties_snapshot(id, provider_id, &native)?;
        Ok(Self {
            native,
            properties,
            provider_id: provider_id.to_owned(),
        })
    }

    /// Builds the immutable host filesystem property snapshot.
    ///
    /// # Returns
    ///
    /// Properties for the `local-host` identity and current native protocol
    /// support.
    ///
    /// # Panics
    ///
    /// Panics only if the static filesystem identity, `file` scheme, or
    /// internally assembled property set violates a `qubit-fs` invariant.
    fn properties_snapshot(
        id: FileSystemId,
        provider_id: &str,
        native: &native_files::LocalFileSystem,
    ) -> FsResult<FileSystemProperties> {
        let native_protocols = native.protocols();
        let mut capabilities = FileSystemCapabilities::new()
            .with_guaranteed(FileSystemCapability::List)
            .with_guaranteed(FileSystemCapability::Read)
            .with_guaranteed(FileSystemCapability::Write)
            .with_guaranteed(FileSystemCapability::Append)
            .with_guaranteed(FileSystemCapability::CreateDirectory)
            .with_guaranteed(FileSystemCapability::EmptyDirectory)
            .with_guaranteed(FileSystemCapability::Delete)
            .with_guaranteed(FileSystemCapability::RecursiveDelete)
            .with_guaranteed(FileSystemCapability::Rename)
            .with_guaranteed(FileSystemCapability::Copy)
            .with_guaranteed(FileSystemCapability::TempFile)
            .with_guaranteed(FileSystemCapability::TempDirectory);
        if native_protocols.supports_atomic_rename() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::AtomicRename);
        }
        if native_protocols.supports_atomic_replace() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::AtomicReplace)
                .with_conditional(FileSystemCapability::AtomicFileCopy);
        }
        if native_protocols.supports_atomic_temp_persist() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::AtomicTempPersist);
        }
        if native_protocols.supports_durable_rename() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::DurableRename);
        }
        if native_protocols.supports_durable_file_copy() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::DurableFileCopy);
        }
        FileSystemProperties::new(
            FileSystemInfo::new(id, provider_id, PathSemantics::Hierarchical)
                .with_scheme(FILE_SCHEME)?,
            capabilities,
            native_limits(native.limits()),
            PathConstraints::absolute(),
            match native.symlink_policy() {
                native_files::LocalSymlinkPolicy::Reject => {
                    SymlinkPolicy::Reject
                }
                native_files::LocalSymlinkPolicy::FollowWithinScope
                | native_files::LocalSymlinkPolicy::FollowAcrossScope => {
                    SymlinkPolicy::FollowWithinFileSystem
                }
            },
        )
    }

    /// Converts one logical path for the configured native scope.
    fn native_path(&self, path: &Path) -> FsResult<PathBuf> {
        local_path_mapper::native(self.native.scope(), path)
    }

    /// Converts a logical source-target pair for the configured scope.
    fn native_pair(
        &self,
        source: &Path,
        target: &Path,
    ) -> FsResult<(PathBuf, PathBuf)> {
        Ok((self.native_path(source)?, self.native_path(target)?))
    }

    /// Converts a returned native path to its logical representation.
    fn logical_path(
        &self,
        path: &NativePath,
        operation: FsOperation,
    ) -> FsResult<Path> {
        local_path_mapper::logical(self.native.scope(), path, operation)
    }

    /// Reports whether native paths are Rooted descendants.
    #[inline(always)]
    fn is_rooted(&self) -> bool {
        matches!(
            self.native.scope(),
            native_files::LocalFileSystemScope::Rooted
        )
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
    fn info(&self, path: Path) -> OpenedFileInfo {
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
        path: &Path,
    ) -> FsError {
        error_mapper::map(error, operation, path, None, &self.provider_id)
    }
}

/// Maps native authority path limits into portable logical path limits.
#[inline(always)]
fn native_limits(
    limits: native_files::LocalFileSystemLimits,
) -> FileSystemLimits {
    FileSystemLimits::unknown()
        .with_max_path_text_bytes(native_limit(limits.max_path_bytes()))
        .with_max_component_text_bytes(native_limit(
            limits.max_file_name_bytes(),
        ))
}

/// Preserves finite, path-dependent, and unavailable native limit semantics.
#[inline(always)]
const fn native_limit(limit: native_files::SizeLimit) -> FileSystemLimit {
    match limit {
        native_files::SizeLimit::Maximum(value) => {
            FileSystemLimit::Maximum(value)
        }
        native_files::SizeLimit::VariesByPath => FileSystemLimit::Unknown,
        native_files::SizeLimit::Unknown => FileSystemLimit::Unknown,
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
        let path = self.native_path(request.path())?;
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
        let path = self.native_path(request.path())?;
        let options =
            local_options_mapper::list(request.options(), self.native.scope())?;
        let rooted = self.is_rooted();
        self.native
            .list(&path, &options)
            .map(|value| {
                let stream = if rooted {
                    LocalDirectoryStreamSpi::rooted(
                        value,
                        request.path().clone(),
                        request.options(),
                        &self.provider_id,
                    )
                } else {
                    LocalDirectoryStreamSpi::host(
                        value,
                        request.path().clone(),
                        request.options(),
                        &self.provider_id,
                    )
                };
                OpenedDirectoryStream::new(Box::new(stream))
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
        let path = self.native_path(request.path())?;
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
        let path = self.native_path(request.path())?;
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
        let path = self.native_path(request.path())?;
        let options =
            local_options_mapper::create_directory(request.options())?;
        self.native
            .create_directory(&path, &options)
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
        let path = self.native_path(request.path())?;
        let options = local_options_mapper::delete(request.options());
        self.native
            .delete_file(&path, &options)
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
        let path = self.native_path(request.path())?;
        let options = local_options_mapper::delete(request.options());
        self.native
            .delete_directory(&path, &options)
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
        let options = match local_options_mapper::copy(
            request.options(),
            self.native.scope(),
        ) {
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
        let (source, target) = self
            .native_pair(request.source(), request.target())
            .map_err(error_mapper::copy_path_error)?;
        self.native
            .copy(&source, &target, &options)
            .map(|value| {
                CopyAttempt::Completed(local_outcome_mapper::copy(value))
            })
            .map_err(|error| {
                let state = error.state();
                let stats = *error.partial_stats();
                let failure_path =
                    error.failed_source_path().and_then(|path| {
                        local_path_mapper::logical(
                            self.native.scope(),
                            path,
                            FsOperation::Copy,
                        )
                        .ok()
                    });
                let failure_target =
                    error.failed_target_path().and_then(|path| {
                        local_path_mapper::logical(
                            self.native.scope(),
                            path,
                            FsOperation::Copy,
                        )
                        .ok()
                    });
                SpiCopyFailure::new(
                    error_mapper::copy_failure(
                        error,
                        request.source(),
                        request.target(),
                        failure_path.as_ref(),
                        failure_target.as_ref(),
                        &self.provider_id,
                    ),
                    local_outcome_mapper::copy_failure_state(state),
                    CopyStats {
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
    ) -> Result<RenameOutcome, SpiRenameFailure> {
        let (source, target) = self
            .native_pair(request.source(), request.target())
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
            .parent()
            .map(|path| self.native_path(path))
            .transpose()?;
        let mut options = native_files::LocalTempFileOptions::new()
            .with_prefix(request.options().prefix())
            .with_suffix(request.options().suffix());
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        if request.options().creates_parent() {
            options = options.with_create_parent();
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
        let path = self.logical_path(value.path(), FsOperation::CreateTemp)?;
        Ok(OpenedTempFile::new(
            self.info(path)
                .with_metadata(FileMetadata::new(FileKind::File)),
            Box::new(LocalTempResourceSpi::file(
                value,
                self.is_rooted(),
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
            .parent()
            .map(|path| self.native_path(path))
            .transpose()?;
        let mut options = native_files::LocalTempDirectoryOptions::new()
            .with_prefix(request.options().prefix())
            .with_suffix(request.options().suffix());
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        if request.options().creates_parent() {
            options = options.with_create_parent();
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
        let path = self.logical_path(value.path(), FsOperation::CreateTemp)?;
        Ok(OpenedTempDirectory::new(
            self.info(path)
                .with_metadata(FileMetadata::new(FileKind::Directory)),
            Box::new(LocalTempResourceSpi::directory(
                value,
                self.is_rooted(),
                self.provider_id.clone(),
            )),
        ))
    }
}
