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

use qubit_fs::copy::CopyFailureState;
use qubit_fs::copy::CopyStats;
use qubit_fs::directory::CreateDirectoryOutcome;
use qubit_fs::directory::DeleteOutcome;
use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_fs::metadata::FileKind;
use qubit_fs::metadata::FileMetadata;
use qubit_fs::metadata::FileSystemCapabilities;
use qubit_fs::metadata::FileSystemCapability;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::metadata::FileSystemInfo;
use qubit_fs::metadata::FileSystemLimit;
use qubit_fs::metadata::FileSystemLimits;
use qubit_fs::metadata::FileSystemProperties;
use qubit_fs::metadata::OpenedFileInfo;
use qubit_fs::metadata::SymlinkPolicy;
use qubit_fs::path::Path;
use qubit_fs::path::PathConstraints;
use qubit_fs::path::PathSemantics;
use qubit_fs::rename::RenameOutcome;
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
use qubit_fs::spi::ProviderOperation;
use qubit_fs::spi::ProviderOperations;
use qubit_fs::spi::ProviderProperties;
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
use crate::LocalResourcePolicy;
use crate::constants::FILE_SCHEME;
use crate::constants::LOCAL_PROVIDER_ID;
use crate::path::local_path_mapper;

/// Host or Rooted implementation of the synchronous local filesystem SPI.
#[must_use]
pub struct LocalFileSystemSpi {
    /// Configured Host or Rooted native filesystem engine.
    native: native_files::LocalFileSystem,
    /// Immutable capability support, limits, path rules, and provider
    /// identity.
    properties: FileSystemProperties,
    /// Provider identity attached to every translated failure.
    provider_id: String,
    /// Immutable provider resource policy, separate from native mutable
    /// defaults.
    resource_policy: LocalResourcePolicy,
}

impl LocalFileSystemSpi {
    /// Creates the fixed host filesystem implementation.
    ///
    /// # Returns
    ///
    /// A host SPI with capability support derived from the native backend.
    ///
    /// # Errors
    ///
    /// Returns a provider error when portable properties cannot be assembled.
    #[inline(always)]
    pub fn new(resource_policy: LocalResourcePolicy) -> FsResult<Self> {
        let native =
            native_files::LocalFileSystem::host().map_err(|error| {
                error_mapper::map_without_path(
                    error,
                    FsOperation::Provider,
                    "cannot capture the host local filesystem",
                    LOCAL_PROVIDER_ID,
                )
            })?;
        Self::from_native(
            FileSystemId::new("local-host")
                .expect("static filesystem identity is valid"),
            LOCAL_PROVIDER_ID,
            native,
            resource_policy,
        )
    }

    /// Opens a Rooted filesystem with the default local provider identity.
    ///
    /// # Errors
    ///
    /// Returns a provider error when the native authority or portable
    /// properties cannot be constructed.
    pub fn rooted(
        id: FileSystemId,
        root: &NativePath,
        resource_policy: LocalResourcePolicy,
    ) -> FsResult<Self> {
        Self::rooted_with_provider_id(
            id,
            LOCAL_PROVIDER_ID,
            root,
            resource_policy,
        )
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
        resource_policy: LocalResourcePolicy,
    ) -> FsResult<Self> {
        let native = Self::open_rooted(root, provider_id)?;
        Self::from_native(id, provider_id, native, resource_policy)
    }

    /// Maps one rooted constructor into provider context.
    fn open_rooted(
        root: &NativePath,
        provider_id: &str,
    ) -> FsResult<native_files::LocalFileSystem> {
        native_files::LocalFileSystem::rooted(root).map_err(|error| {
            FsError::with_source(
                FsErrorKind::ProviderUnavailable,
                FsOperation::Provider,
                "cannot open rooted local filesystem",
                error,
            )
            .with_provider(provider_id)
        })
    }

    /// Builds the SPI around one fully configured native instance.
    fn from_native(
        id: FileSystemId,
        provider_id: &str,
        native: native_files::LocalFileSystem,
        resource_policy: LocalResourcePolicy,
    ) -> FsResult<Self> {
        let properties = Self::properties_snapshot(id, provider_id, &native)?;
        Ok(Self {
            native,
            properties,
            provider_id: provider_id.to_owned(),
            resource_policy,
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
        let native_capabilities = native.capabilities();
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
        if native_capabilities.supports_atomic_rename() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::AtomicRename);
        }
        if native_capabilities.supports_atomic_replace() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::AtomicReplace)
                .with_conditional(FileSystemCapability::AtomicFileCopy);
        }
        if native_capabilities.supports_atomic_temp_persist() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::AtomicTempPersist);
        }
        if native_capabilities.supports_durable_rename() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::DurableRename);
        }
        if native_capabilities.supports_durable_file_copy() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::DurableFileCopy);
        }
        if native_capabilities.supports_durable_write() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::DurableWrite);
        }
        FileSystemProperties::new(
            FileSystemInfo::new(id, provider_id, PathSemantics::Hierarchical)
                .with_scheme(FILE_SCHEME)?,
            capabilities,
            native_limits(native.limits()),
            PathConstraints::absolute(),
            match native.symlink_policy() {
                native_files::policy::LocalSymlinkPolicy::Reject => {
                    SymlinkPolicy::Reject
                }
                native_files::policy::LocalSymlinkPolicy::FollowWithinScope
                | native_files::policy::LocalSymlinkPolicy::FollowAcrossScope => {
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
            native_files::path::LocalFileSystemScope::Rooted
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
    limits: native_files::capability::LocalFileSystemLimits,
) -> FileSystemLimits {
    if limits.length_unit()
        != native_files::capability::LocalPathLengthUnit::Bytes
    {
        return FileSystemLimits::unknown();
    }
    FileSystemLimits::unknown()
        .with_max_path_text_bytes(native_limit(limits.max_path_length()))
        .with_max_component_text_bytes(native_limit(
            limits.max_component_length(),
        ))
}

/// Preserves finite, path-dependent, and unavailable native limit semantics.
#[inline(always)]
const fn native_limit(
    limit: native_files::capability::SizeLimit,
) -> FileSystemLimit {
    match limit {
        native_files::capability::SizeLimit::Maximum(value) => {
            FileSystemLimit::Maximum(value)
        }
        native_files::capability::SizeLimit::VariesByPath => {
            FileSystemLimit::Unknown
        }
        native_files::capability::SizeLimit::Unknown => {
            FileSystemLimit::Unknown
        }
    }
}

impl FileSystemSpi for LocalFileSystemSpi {
    /// Returns the immutable host filesystem properties.
    ///
    /// # Returns
    ///
    /// A snapshot of host identity, capabilities, limits, and path rules.
    #[inline(always)]
    fn properties(&self) -> ProviderProperties {
        ProviderProperties::new(
            self.properties.info().clone(),
            ProviderOperations::new()
                .with(ProviderOperation::Stat)
                .with(ProviderOperation::List)
                .with(ProviderOperation::OpenReader)
                .with(ProviderOperation::OpenWriter)
                .with(ProviderOperation::CreateDirectory)
                .with(ProviderOperation::DeleteFile)
                .with(ProviderOperation::DeleteDirectory)
                .with(ProviderOperation::TryCopy)
                .with(ProviderOperation::Rename)
                .with(ProviderOperation::CreateTempFile)
                .with(ProviderOperation::CreateTempDirectory),
            self.properties.capabilities(),
            *self.properties.limits(),
            self.properties.path_constraints().clone(),
            self.properties.symlink_policy(),
        )
        .expect("local provider properties remain valid")
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
        let options = local_options_mapper::list(
            request.options(),
            self.native.scope(),
            self.resource_policy.list_options(),
        )?;
        let rooted = self.is_rooted();
        self.native
            .list_with_options(&path, &options)
            .map(|value| {
                let stream = if rooted {
                    LocalDirectoryStreamSpi::rooted(
                        value,
                        request.options(),
                        &self.provider_id,
                    )
                } else {
                    LocalDirectoryStreamSpi::host(
                        value,
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
        let mut options = local_options_mapper::read(request.options());
        if let Some(timeout) = self.resource_policy.open_retry_timeout() {
            options = options.with_open_retry_timeout(timeout);
        }
        self.native
            .open_reader_with_options(&path, &options)
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
        let mut options = local_options_mapper::write(request.options())?;
        if let Some(timeout) = self.resource_policy.open_retry_timeout() {
            options = options.with_open_retry_timeout(timeout);
        }
        self.native
            .open_writer_with_options(&path, &options)
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
            .create_directory_with_options(&path, &options)
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
            .delete_file_with_options(&path, &options)
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
            .delete_directory_with_options(&path, &options)
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
            self.resource_policy.copy_options(),
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
            .copy_with_options(&source, &target, &options)
            .map(|value| {
                CopyAttempt::Completed(local_outcome_mapper::copy(value))
            })
            .map_err(|error| {
                let state = local_outcome_mapper::copy_failure_state(error.state());
                let stats = *error.partial_stats();
                let partial_stats = CopyStats {
                    files: stats.files(),
                    directories: stats.directories(),
                    bytes: stats.bytes(),
                    skipped: stats.skipped(),
                    overwritten: stats.overwritten(),
                    ..Default::default()
                };
                let failure_path = error
                    .failed_source_path()
                    .map(|path| {
                        local_path_mapper::logical(
                            self.native.scope(),
                            path,
                            FsOperation::Copy,
                        )
                    })
                    .transpose();
                let failure_target = error
                    .failed_target_path()
                    .map(|path| {
                        local_path_mapper::logical(
                            self.native.scope(),
                            path,
                            FsOperation::Copy,
                        )
                    })
                    .transpose();
                if failure_path.is_err() || failure_target.is_err() {
                    let mapped = FsError::with_source(
                        FsErrorKind::ProviderContractViolation,
                        FsOperation::Copy,
                        "local copy failure contained an unrepresentable native failure path",
                        error,
                    )
                    .with_path(request.source().clone())
                    .with_target(request.target().clone())
                    .with_provider(&self.provider_id)
                    .with_effect_state(error_mapper::copy_effect_state(state));
                    return SpiCopyFailure::new(mapped, state, partial_stats);
                }
                let failure_path = failure_path.expect("checked successful failure-path conversion");
                let failure_target = failure_target.expect("checked successful failure-target conversion");
                SpiCopyFailure::new(
                    error_mapper::copy_failure(
                        error,
                        request.source(),
                        request.target(),
                        failure_path.as_ref(),
                        failure_target.as_ref(),
                        &self.provider_id,
                    )
                    .with_effect_state(error_mapper::copy_effect_state(state)),
                    state,
                    partial_stats,
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
            .rename_with_options(&source, &target, &options)
            .map(|value| {
                local_outcome_mapper::rename(
                    value,
                    request.source(),
                    request.target(),
                )
            })
            .map_err(|error| {
                let (error, state) = error.into_parts();
                let state = local_outcome_mapper::rename_failure_state(state);
                SpiRenameFailure::new(
                    error_mapper::map(
                        error,
                        FsOperation::Rename,
                        request.source(),
                        Some(request.target()),
                        &self.provider_id,
                    )
                    .with_effect_state(
                        error_mapper::rename_effect_state(state),
                    ),
                    state,
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
        let mut options = native_files::options::LocalTempFileOptions::new()
            .with_prefix(request.options().prefix())
            .with_suffix(request.options().suffix());
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        if request.options().creates_parent() {
            options = options.with_create_parent();
        }
        if let Some(max_attempts) = self.resource_policy.temp_max_attempts() {
            options = options.with_max_attempts(max_attempts.get());
        }
        let value = self
            .native
            .create_temp_file_with_options(&options)
            .map_err(|error| match request.options().parent() {
                Some(parent) => error_mapper::map(
                    error,
                    FsOperation::CreateTemp,
                    parent,
                    None,
                    &self.provider_id,
                ),
                None => error_mapper::map_without_path(
                    error,
                    FsOperation::CreateTemp,
                    "native temporary file creation failed",
                    &self.provider_id,
                ),
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
        let mut options =
            native_files::options::LocalTempDirectoryOptions::new()
                .with_prefix(request.options().prefix())
                .with_suffix(request.options().suffix());
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        if request.options().creates_parent() {
            options = options.with_create_parent();
        }
        if let Some(max_attempts) = self.resource_policy.temp_max_attempts() {
            options = options.with_max_attempts(max_attempts.get());
        }
        let value = self
            .native
            .create_temp_directory_with_options(&options)
            .map_err(|error| match request.options().parent() {
                Some(parent) => error_mapper::map(
                    error,
                    FsOperation::CreateTemp,
                    parent,
                    None,
                    &self.provider_id,
                ),
                None => error_mapper::map_without_path(
                    error,
                    FsOperation::CreateTemp,
                    "native temporary directory creation failed",
                    &self.provider_id,
                ),
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

#[cfg(test)]
mod tests {
    use qubit_fs::metadata::FileSystemLimit;
    use qubit_local_files::capability::LocalFileSystemLimits;
    use qubit_local_files::capability::LocalPathLengthUnit;
    use qubit_local_files::capability::SizeLimit;

    use super::native_limits;

    /// Verifies byte-native limits map directly into portable text budgets.
    #[test]
    fn test_native_limits_maps_byte_units() {
        let limits = LocalFileSystemLimits::new(
            SizeLimit::Maximum(4096),
            SizeLimit::Maximum(255),
            LocalPathLengthUnit::Bytes,
        );

        let mapped = native_limits(limits);

        assert_eq!(
            FileSystemLimit::Maximum(4096),
            mapped.max_path_text_bytes()
        );
        assert_eq!(
            FileSystemLimit::Maximum(255),
            mapped.max_component_text_bytes()
        );
    }

    /// Verifies UTF-16 limits are not misrepresented as portable byte budgets.
    #[test]
    fn test_native_limits_does_not_convert_utf16_units_to_bytes() {
        let limits = LocalFileSystemLimits::new(
            SizeLimit::Unknown,
            SizeLimit::Maximum(255),
            LocalPathLengthUnit::Utf16CodeUnits,
        );

        let mapped = native_limits(limits);

        assert_eq!(FileSystemLimit::Unknown, mapped.max_path_text_bytes());
        assert_eq!(FileSystemLimit::Unknown, mapped.max_component_text_bytes());
    }
}
