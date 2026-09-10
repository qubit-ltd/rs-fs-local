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

use std::num::NonZeroUsize;
use std::path::Path as NativePath;
use std::path::PathBuf;
use std::time::Duration;

use qubit_fs::copy::CopyFailureState;
use qubit_fs::copy::CopyStats;
use qubit_fs::directory::CreateDirectoryOutcome;
use qubit_fs::directory::DeleteOutcome;
use qubit_fs::error::FsEffectState;
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
use qubit_fs::metadata::FileSystemLimits;
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
use super::local_temp_path_projection::LocalTempPathProjection;
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
    properties: ProviderProperties,
    /// Provider identity attached to every translated failure.
    provider_id: String,
    /// Retry timeout not represented by resolved portable read requests.
    open_retry_timeout: Option<Duration>,
    /// Temporary-name attempt limit not represented by native defaults.
    temp_max_attempts: Option<NonZeroUsize>,
}

impl LocalFileSystemSpi {
    /// Creates the fixed host filesystem implementation.
    ///
    /// # Returns
    ///
    /// A host SPI with capability support derived from the native backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use qubit_fs_local::spi::LocalFileSystemSpi;
    /// use qubit_fs_local::LocalResourcePolicy;
    ///
    /// let _spi = LocalFileSystemSpi::new(LocalResourcePolicy::unbounded())?;
    /// # Ok::<(), qubit_fs::error::FsError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a provider error when portable properties cannot be assembled.
    #[inline(always)]
    pub fn new(resource_policy: LocalResourcePolicy) -> FsResult<Self> {
        let native = native_files::LocalFileSystem::host().map_err(|error| {
            error_mapper::map_without_path(
                error,
                FsOperation::Provider,
                "cannot capture the host local filesystem",
                LOCAL_PROVIDER_ID,
            )
        })?;
        Self::from_native(
            FileSystemId::new("local-host").expect("static filesystem identity is valid"),
            LOCAL_PROVIDER_ID,
            native,
            resource_policy,
        )
    }

    /// Opens a Rooted filesystem with the default local provider identity.
    ///
    /// # Parameters
    ///
    /// - `id`: Stable filesystem identity exposed by the SPI.
    /// - `root`: Native directory retained as filesystem authority.
    /// - `resource_policy`: Recursive resource and lifecycle policy.
    ///
    /// # Returns
    ///
    /// A rooted local SPI retaining the opened native authority.
    ///
    /// # Errors
    ///
    /// Returns a provider error when the native authority or portable
    /// properties cannot be constructed.
    pub fn rooted(id: FileSystemId, root: &NativePath, resource_policy: LocalResourcePolicy) -> FsResult<Self> {
        Self::rooted_with_provider_id(id, LOCAL_PROVIDER_ID, root, resource_policy)
    }

    /// Opens a Rooted filesystem with an explicit provider identity.
    ///
    /// # Parameters
    ///
    /// - `id`: Stable filesystem identity exposed by the SPI.
    /// - `provider_id`: Provider identifier attached to failures.
    /// - `root`: Native directory retained as filesystem authority.
    /// - `resource_policy`: Recursive resource and lifecycle policy.
    ///
    /// # Returns
    ///
    /// A rooted local SPI retaining the opened native authority.
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
    fn open_rooted(root: &NativePath, provider_id: &str) -> FsResult<native_files::LocalFileSystem> {
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
        mut native: native_files::LocalFileSystem,
        resource_policy: LocalResourcePolicy,
    ) -> FsResult<Self> {
        native
            .set_default_delete_options(resource_policy.delete_options())
            .map_err(|error| {
                error_mapper::map_without_path(
                    error,
                    FsOperation::Provider,
                    "invalid native deletion resource policy",
                    provider_id,
                )
            })?;
        native
            .set_default_list_options(resource_policy.list_options())
            .map_err(|error| {
                error_mapper::map_without_path(
                    error,
                    FsOperation::Provider,
                    "invalid native listing resource policy",
                    provider_id,
                )
            })?;
        native
            .set_default_copy_options(resource_policy.copy_options())
            .map_err(|error| {
                error_mapper::map_without_path(
                    error,
                    FsOperation::Provider,
                    "invalid native copy resource policy",
                    provider_id,
                )
            })?;
        let properties = Self::properties_snapshot(id, provider_id, &native)?;
        Ok(Self {
            native,
            properties,
            provider_id: provider_id.to_owned(),
            open_retry_timeout: resource_policy.open_retry_timeout(),
            temp_max_attempts: resource_policy.temp_max_attempts(),
        })
    }

    /// Builds the immutable provider property snapshot.
    ///
    /// # Returns
    ///
    /// Properties for the supplied identity and current native protocol
    /// support.
    ///
    /// # Errors
    ///
    /// Returns an invalid-options error if the assembled provider properties
    /// violate a shared `qubit-fs` invariant.
    fn properties_snapshot(
        id: FileSystemId,
        provider_id: &str,
        native: &native_files::LocalFileSystem,
    ) -> FsResult<ProviderProperties> {
        let native_capabilities = native.capabilities();
        let mut capabilities = FileSystemCapabilities::new()
            .with_guaranteed(FileSystemCapability::List)
            .with_guaranteed(FileSystemCapability::Read)
            .with_conditional(FileSystemCapability::RangeRead)
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
            capabilities = capabilities.with_conditional(FileSystemCapability::AtomicRename);
        }
        if native_capabilities.supports_atomic_replace() {
            capabilities = capabilities
                .with_conditional(FileSystemCapability::AtomicReplace)
                .with_conditional(FileSystemCapability::AtomicFileCopy);
        }
        if native_capabilities.can_attempt_atomic_temp_persist() {
            capabilities = capabilities.with_conditional(FileSystemCapability::AtomicTempPersist);
        }
        if native_capabilities.supports_durable_rename() {
            capabilities = capabilities.with_conditional(FileSystemCapability::DurableRename);
        }
        if native_capabilities.supports_durable_file_copy() {
            capabilities = capabilities.with_conditional(FileSystemCapability::DurableFileCopy);
        }
        if native_capabilities.supports_durable_write() {
            capabilities = capabilities.with_conditional(FileSystemCapability::DurableWrite);
        }
        ProviderProperties::new(
            FileSystemInfo::new(id, provider_id, PathSemantics::Hierarchical).with_scheme(FILE_SCHEME)?,
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
            capabilities,
            // Native path units and canonical escaped text have different
            // lengths. Native validity and size limits remain enforced by
            // the native backend.
            FileSystemLimits::unknown(),
            PathConstraints::absolute(),
            match native.symlink_policy() {
                native_files::policy::LocalSymlinkPolicy::Reject => SymlinkPolicy::Reject,
                native_files::policy::LocalSymlinkPolicy::FollowWithinScope
                | native_files::policy::LocalSymlinkPolicy::FollowAcrossScope => SymlinkPolicy::FollowWithinFileSystem,
            },
        )
    }

    /// Converts one logical path for the configured native scope.
    fn native_path(&self, path: &Path) -> FsResult<PathBuf> {
        local_path_mapper::native(self.native.scope(), path)
    }

    /// Converts a logical source-target pair for the configured scope.
    fn native_pair(&self, source: &Path, target: &Path) -> FsResult<(PathBuf, PathBuf)> {
        Ok((self.native_path(source)?, self.native_path(target)?))
    }

    /// Converts a returned native path to its logical representation.
    fn logical_path(&self, path: &NativePath, operation: FsOperation) -> FsResult<Path> {
        local_path_mapper::logical(self.native.scope(), path, operation)
    }

    /// Reports whether native paths are Rooted descendants.
    #[inline(always)]
    fn is_rooted(&self) -> bool {
        matches!(self.native.scope(), native_files::path::LocalFileSystemScope::Rooted)
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
    fn map(&self, error: native_files::LocalFileError, operation: FsOperation, path: &Path) -> FsError {
        error_mapper::map(error, operation, path, None, &self.provider_id)
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
            .map(|value| StatResponse::new(request.path().clone(), local_outcome_mapper::metadata(value)))
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
    fn list(&self, request: ListRequest<'_>) -> FsResult<OpenedDirectoryStream> {
        let logical_path = request.scope().path().ok_or_else(|| {
            FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::List,
                "local listing requires a directory path",
            )
            .with_provider(&self.provider_id)
        })?;
        let path = self.native_path(logical_path)?;
        let options = local_options_mapper::list(
            request.options(),
            self.native.scope(),
            *self.native.default_list_options(),
        )?;
        let rooted = self.is_rooted();
        self.native
            .list_with_options(&path, &options)
            .map(|value| {
                let stream = if rooted {
                    LocalDirectoryStreamSpi::rooted(value, request.options(), &self.provider_id)
                } else {
                    LocalDirectoryStreamSpi::host(value, request.options(), &self.provider_id)
                };
                OpenedDirectoryStream::new(Box::new(stream))
            })
            .map_err(|error| self.map(error, FsOperation::List, logical_path))
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
    fn open_reader(&self, request: OpenReaderRequest<'_>) -> FsResult<OpenedReader> {
        let path = self.native_path(request.path())?;
        let mut options = local_options_mapper::read(request.options());
        if let Some(timeout) = self.open_retry_timeout {
            options = options.with_open_retry_timeout(timeout);
        }
        let reader = self
            .native
            .open_reader_with_options(&path, &options)
            .map_err(|error| self.map(error, FsOperation::OpenReader, request.path()))?;
        let info = self
            .info(request.path().clone())
            .with_metadata(local_outcome_mapper::metadata(reader.metadata().clone()));
        let window = request.options().options();
        let reader =
            super::local_range_reader::LocalRangeReader::new(reader, window.offset().unwrap_or(0), window.length())
                .map_err(|error| {
                    FsError::with_source(
                        FsErrorKind::Io,
                        FsOperation::OpenReader,
                        "local range seek failed",
                        error,
                    )
                    .with_path(request.path().clone())
                    .with_provider(self.properties.info().provider_id())
                })?;
        Ok(OpenedReader::new(info, Box::new(reader)))
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
    fn open_writer(&self, request: OpenWriterRequest<'_>) -> FsResult<OpenedWriter> {
        let path = self
            .native_path(request.path())
            .map_err(|error| error.with_effect_state(FsEffectState::Unchanged))?;
        let mut options = local_options_mapper::write(request.options())
            .map_err(|error| error.with_effect_state(FsEffectState::Unchanged))?;
        if let Some(timeout) = self.open_retry_timeout {
            options = options.with_open_retry_timeout(timeout);
        }
        self.native
            .open_writer_with_options(&path, &options)
            .map(|value| {
                OpenedWriter::new(
                    self.info(request.path().clone()),
                    Box::new(LocalFileWriterSpi::new(value, self.provider_id.clone())),
                )
            })
            .map_err(|error| self.map(error, FsOperation::OpenWriter, request.path()))
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
    fn create_directory(&self, request: CreateDirectoryRequest<'_>) -> FsResult<CreateDirectoryOutcome> {
        let path = self.native_path(request.path())?;
        let options = local_options_mapper::create_directory(request.options())?;
        self.native
            .create_directory_with_options(&path, &options)
            .map(|value| CreateDirectoryOutcome::new(!value.created()))
            .map_err(|error| self.map(error, FsOperation::CreateDir, request.path()))
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
    fn delete_file(&self, request: DeleteFileRequest<'_>) -> FsResult<DeleteOutcome> {
        let path = self.native_path(request.path())?;
        let options = local_options_mapper::delete(request.options(), *self.native.default_delete_options());
        self.native
            .delete_file_with_options(&path, &options)
            .map(|value| DeleteOutcome::new(!value.deleted()))
            .map_err(|error| self.map(error, FsOperation::Delete, request.path()))
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
    fn delete_directory(&self, request: DeleteDirectoryRequest<'_>) -> FsResult<DeleteOutcome> {
        let path = self.native_path(request.path())?;
        let options = local_options_mapper::delete(request.options(), *self.native.default_delete_options());
        self.native
            .delete_directory_with_options(&path, &options)
            .map(|value| DeleteOutcome::new(!value.deleted()))
            .map_err(|error| self.map(error, FsOperation::Delete, request.path()))
    }

    /// Attempts a native host copy when all requirements are expressible.
    ///
    /// # Parameters
    ///
    /// - `request`: Resolved source, target, and copy policy.
    ///
    /// # Returns
    ///
    /// `Completed` when the configured native copy succeeds. This local
    /// provider does not decline into the facade stream fallback.
    /// Unrepresentable options fail before native copying starts.
    ///
    /// # Errors
    ///
    /// Returns a structured copy failure for path conversion or native copy
    /// errors, preserving publication state and partial statistics.
    fn try_copy(&self, request: CopyRequest<'_>) -> Result<CopyAttempt, SpiCopyFailure> {
        let options = match local_options_mapper::copy(
            request.options(),
            self.native.scope(),
            *self.native.default_copy_options(),
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
            .map(|value| CopyAttempt::Completed(local_outcome_mapper::copy(value)))
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
                    .map(|path| local_path_mapper::logical(self.native.scope(), path, FsOperation::Copy))
                    .transpose();
                let failure_target = error
                    .failed_target_path()
                    .map(|path| local_path_mapper::logical(self.native.scope(), path, FsOperation::Copy))
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
    fn rename(&self, request: RenameRequest<'_>) -> Result<RenameOutcome, SpiRenameFailure> {
        let (source, target) = self
            .native_pair(request.source(), request.target())
            .map_err(error_mapper::rename_path_error)?;
        let options = local_options_mapper::rename(request.options());
        self.native
            .rename_with_options(&source, &target, &options)
            .map(|value| local_outcome_mapper::rename(value, request.source(), request.target()))
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
                    .with_effect_state(error_mapper::rename_effect_state(state)),
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
    fn create_temp_file(&self, request: CreateTempFileRequest) -> FsResult<OpenedTempFile> {
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
        if let Some(max_attempts) = self.temp_max_attempts {
            options = options.with_max_attempts(max_attempts.get());
        }
        let value =
            self.native
                .create_temp_file_with_options(&options)
                .map_err(|error| match request.options().parent() {
                    Some(parent) => error_mapper::map(error, FsOperation::CreateTemp, parent, None, &self.provider_id),
                    None => error_mapper::map_without_path(
                        error,
                        FsOperation::CreateTemp,
                        "native temporary file creation failed",
                        &self.provider_id,
                    ),
                })?;
        let projection = parent
            .as_deref()
            .map(|parent| LocalTempPathProjection::new(parent, value.path()))
            .transpose()?;
        let public_path = projection
            .as_ref()
            .map(|projection| projection.project(value.path(), FsOperation::CreateTemp))
            .transpose()?;
        let path = self.logical_path(public_path.as_deref().unwrap_or(value.path()), FsOperation::CreateTemp)?;
        Ok(OpenedTempFile::new(
            self.info(path).with_metadata(FileMetadata::new(FileKind::File)),
            Box::new(LocalTempResourceSpi::file(
                value,
                self.is_rooted(),
                self.provider_id.clone(),
                projection,
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
    fn create_temp_directory(&self, request: CreateTempDirectoryRequest) -> FsResult<OpenedTempDirectory> {
        let parent = request
            .options()
            .parent()
            .map(|path| self.native_path(path))
            .transpose()?;
        let mut options = native_files::options::LocalTempDirectoryOptions::new()
            .with_prefix(request.options().prefix())
            .with_suffix(request.options().suffix());
        if let Some(parent) = parent.as_deref() {
            options = options.with_parent(parent);
        }
        if request.options().creates_parent() {
            options = options.with_create_parent();
        }
        if let Some(max_attempts) = self.temp_max_attempts {
            options = options.with_max_attempts(max_attempts.get());
        }
        let value = self
            .native
            .create_temp_directory_with_options(&options)
            .map_err(|error| match request.options().parent() {
                Some(parent) => error_mapper::map(error, FsOperation::CreateTemp, parent, None, &self.provider_id),
                None => error_mapper::map_without_path(
                    error,
                    FsOperation::CreateTemp,
                    "native temporary directory creation failed",
                    &self.provider_id,
                ),
            })?;
        let projection = parent
            .as_deref()
            .map(|parent| LocalTempPathProjection::new(parent, value.path()))
            .transpose()?;
        let public_path = projection
            .as_ref()
            .map(|projection| projection.project(value.path(), FsOperation::CreateTemp))
            .transpose()?;
        let path = self.logical_path(public_path.as_deref().unwrap_or(value.path()), FsOperation::CreateTemp)?;
        Ok(OpenedTempDirectory::new(
            self.info(path).with_metadata(FileMetadata::new(FileKind::Directory)),
            Box::new(LocalTempResourceSpi::directory(
                value,
                self.is_rooted(),
                self.provider_id.clone(),
                projection,
            )),
        ))
    }
}

#[cfg(test)]
mod tests {
    use qubit_fs::FileSystem;
    use qubit_fs::copy::CopyOptions;
    use qubit_fs::directory::DeleteOptions;
    use qubit_fs::directory::ListOptions;
    use qubit_fs::directory::ListScope;
    use qubit_fs::metadata::FileSystemId;
    use qubit_fs::path::Path;
    use qubit_fs::spi::ProviderOperation;
    use qubit_fs::spi::ProviderProperties;
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::options::LocalCopyOptions;
    use qubit_local_files::options::LocalDeleteOptions;
    use qubit_local_files::options::LocalListOptions;

    use super::LocalFileSystemSpi;

    /// The local SPI construction path retains a validated provider snapshot.
    #[test]
    fn test_properties_snapshot_returns_provider_properties() {
        let native = LocalFileSystem::host().expect("host native filesystem should construct");
        let id = FileSystemId::new("local-test").expect("test filesystem identity should be valid");
        let snapshot: ProviderProperties = LocalFileSystemSpi::properties_snapshot(id, "local-file", &native)
            .expect("provider properties should validate");

        assert!(snapshot.operations().supports(ProviderOperation::OpenReader));
    }
    /// Complete nonrecursive requests cannot inherit recursive native defaults.
    #[test]
    fn test_request_list_behavior_is_not_inherited_from_native_defaults() {
        let root = tempfile::tempdir().expect("isolated fixture");
        std::fs::create_dir(root.path().join("nested")).expect("nested directory");
        std::fs::write(root.path().join("nested/file"), b"payload").expect("nested file");
        let mut spi = LocalFileSystemSpi::rooted(
            FileSystemId::new("list-request").expect("identity"),
            root.path(),
            crate::LocalResourcePolicy::unbounded(),
        )
        .expect("SPI");
        spi.native
            .set_default_list_options(LocalListOptions::new().with_recursive())
            .expect("native defaults");
        let filesystem = FileSystem::from_spi(spi).expect("facade");
        let mut entries = filesystem
            .list(
                &ListScope::Path((Path::parse("/").expect("root path")).clone()),
                ListOptions::default(),
            )
            .expect("listing");
        assert!(entries.next_entry().expect("first entry").is_some());
        assert!(
            entries.next_entry().expect("end of nonrecursive listing").is_none(),
            "nonrecursive request must exclude nested file"
        );
    }

    /// A complete delete request cannot inherit missing-ok or recursive
    /// defaults.
    #[test]
    fn test_request_delete_behavior_is_not_inherited_from_native_defaults() {
        let root = tempfile::tempdir().expect("isolated fixture");
        std::fs::create_dir(root.path().join("nested")).expect("nested directory");
        std::fs::write(root.path().join("nested/file"), b"payload").expect("nested file");
        let mut spi = LocalFileSystemSpi::rooted(
            FileSystemId::new("delete-request").expect("identity"),
            root.path(),
            crate::LocalResourcePolicy::unbounded(),
        )
        .expect("SPI");
        spi.native
            .set_default_delete_options(LocalDeleteOptions::new().with_recursive().with_missing_ok())
            .expect("native defaults");
        let filesystem = FileSystem::from_spi(spi).expect("facade");
        assert!(
            filesystem
                .delete_directory(&Path::parse("/absent").expect("missing path"), DeleteOptions::default())
                .is_err()
        );
        assert!(
            filesystem
                .delete_directory(&Path::parse("/nested").expect("nested path"), DeleteOptions::default())
                .is_err()
        );
        assert!(root.path().join("nested/file").exists());
    }

    /// Copy requests own parent creation even if the native default enables it.
    #[test]
    fn test_request_copy_behavior_is_not_inherited_from_native_defaults() {
        let root = tempfile::tempdir().expect("isolated fixture");
        std::fs::write(root.path().join("source"), b"payload").expect("source file");
        let mut spi = LocalFileSystemSpi::rooted(
            FileSystemId::new("copy-request").expect("identity"),
            root.path(),
            crate::LocalResourcePolicy::unbounded(),
        )
        .expect("SPI");
        spi.native
            .set_default_copy_options(LocalCopyOptions::new().with_tree_source().with_create_parent())
            .expect("native defaults");
        let filesystem = FileSystem::from_spi(spi).expect("facade");
        let result = filesystem.copy(
            &Path::parse("/source").expect("source path"),
            &Path::parse("/absent/target").expect("target path"),
            CopyOptions::file().with_create_parent(false),
        );
        assert!(result.is_err(), "request must not create missing parents");
        assert!(!root.path().join("absent").exists());
    }
}
