// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Registry provider for host and rooted local filesystems.

use std::path::Path;

use qubit_fs::{
    FileSystemId,
    FsError,
    FsErrorKind,
    FsOperation,
    Path as FsPath,
    Uri,
};
use qubit_fs_registry::{
    FileSystemConfig,
    FileSystemResolution,
    FileSystemSpec,
};
use qubit_spi::error::ProviderFailure;
use qubit_spi::{
    ProviderDescriptor,
    ProviderId,
    ProviderMetadata,
    ServiceProvider,
};

use crate::LocalFileSystems;
use crate::constants::{
    FILE_SCHEME,
    LOCAL_PROVIDER_ID,
};

use super::internal::LocalProviderMode;
use super::local_file_uri_path;

/// Creates local filesystem resolutions for accepted `file:` configurations.
#[derive(Clone)]
#[must_use]
pub struct LocalFileSystemProvider {
    /// Authority mode used for every configuration resolved by this provider.
    mode: LocalProviderMode,
    /// Descriptor used to register and validate this provider instance.
    descriptor: Option<ProviderDescriptor>,
}

impl LocalFileSystemProvider {
    /// Creates a host-wide local provider.
    ///
    /// # Returns
    ///
    /// A provider that resolves absolute `file:` paths against the process
    /// host filesystem.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            mode: LocalProviderMode::Host,
            descriptor: None,
        }
    }

    /// Creates a provider restricted to `root` with the supplied stable
    /// identity.
    ///
    /// # Parameters
    ///
    /// - `id`: Stable identity exposed by each resolved rooted filesystem.
    /// - `root`: Native directory retained as the provider authority.
    ///
    /// # Returns
    ///
    /// A provider that resolves accepted paths below the opened `root`.
    #[inline]
    pub fn rooted(id: FileSystemId, root: &Path) -> Result<Self, FsError> {
        Self::rooted_with_descriptor(default_descriptor(), id, root)
    }

    /// Creates a rooted provider with caller-specified registration metadata.
    ///
    /// The descriptor ID is also exposed by the configured filesystem, so
    /// multiple rooted local providers can coexist in one registry when they
    /// use distinct IDs and aliases.
    pub fn rooted_with_descriptor(
        descriptor: ProviderDescriptor,
        id: FileSystemId,
        root: &Path,
    ) -> Result<Self, FsError> {
        LocalFileSystems::rooted_with_provider_id(
            id,
            descriptor.id().as_str(),
            root,
        )
        .map(|file_system| Self {
            mode: LocalProviderMode::Rooted { file_system },
            descriptor: Some(descriptor),
        })
    }

    /// Validates and decodes a registry configuration into a logical path and
    /// URI.
    ///
    /// # Parameters
    ///
    /// - `config`: Registry configuration supplied to this provider.
    ///
    /// # Returns
    ///
    /// The decoded absolute logical path and its parsed `file:` URI.
    ///
    /// # Errors
    ///
    /// Returns an invalid-configuration failure when the configuration
    /// contains options, metadata, credentials, a non-`file` scheme, a remote
    /// authority, a query, malformed URI text, or a non-absolute path.
    fn decode_config(
        config: &FileSystemConfig,
    ) -> Result<(FsPath, Uri), ProviderFailure<FsError>> {
        if !config.options().is_empty()
            || !config.metadata().is_empty()
            || config.credential().is_some()
        {
            return Err(invalid_options(
                "local filesystem provider does not support provider options, metadata, or credentials",
            ));
        }
        let uri = config
            .uri()
            .try_to_uri()
            .map_err(ProviderFailure::invalid_configuration)?;
        if uri.scheme() != FILE_SCHEME {
            return Err(invalid_options(
                "local filesystem provider requires the file URI scheme",
            ));
        }
        if uri
            .authority()
            .is_some_and(|authority| !authority.is_empty())
        {
            return Err(invalid_options(
                "local filesystem provider does not support remote URI authorities",
            ));
        }
        if uri.query().is_some() {
            return Err(invalid_options(
                "local filesystem provider does not support URI queries",
            ));
        }
        let path = local_file_uri_path::decode(uri.path())?;
        if !path.is_absolute() {
            return Err(invalid_path("local file URI path must be absolute"));
        }
        let canonical = local_file_uri_path::canonical_uri(&path)?;
        Ok((path, canonical))
    }
}

impl Default for LocalFileSystemProvider {
    /// Creates the default host-wide provider.
    ///
    /// # Returns
    ///
    /// The same host-wide provider produced by [`Self::new`].
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderMetadata for LocalFileSystemProvider {
    /// Returns the stable local-provider descriptor and the `file` alias.
    ///
    /// # Returns
    ///
    /// Metadata identifying the provider as `local-file` with the `file`
    /// alias.
    #[inline]
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone().unwrap_or_else(default_descriptor)
    }
}

/// Builds the default local provider descriptor from shared identities.
#[inline]
fn default_descriptor() -> ProviderDescriptor {
    ProviderDescriptor::new(
        ProviderId::new(LOCAL_PROVIDER_ID)
            .expect("static provider identity is valid"),
    )
    .with_aliases([FILE_SCHEME])
    .expect("static provider alias is valid")
}

impl ServiceProvider<FileSystemSpec> for LocalFileSystemProvider {
    /// Resolves one accepted `file:` configuration to a concrete facade.
    ///
    /// # Parameters
    ///
    /// - `config`: Provider configuration to validate and resolve.
    ///
    /// # Returns
    ///
    /// A concrete filesystem, decoded logical path, and canonical URI.
    ///
    /// # Errors
    ///
    /// Returns an invalid-configuration failure for unsupported configuration
    /// shapes or an initialization failure when the selected local filesystem
    /// cannot be opened or assembled.
    fn create_configured(
        &self,
        config: &FileSystemConfig,
    ) -> Result<FileSystemResolution, ProviderFailure<FsError>> {
        let (path, uri) = Self::decode_config(config)?;
        let file_system = match &self.mode {
            LocalProviderMode::Host => LocalFileSystems::host(),
            LocalProviderMode::Rooted { file_system } => {
                Ok(file_system.clone())
            }
        }
        .map_err(ProviderFailure::initialization_failed)?;
        FileSystemResolution::try_new(file_system, path, uri)
            .map_err(ProviderFailure::initialization_failed)
    }
}

/// Builds an invalid-options provider failure.
///
/// # Parameters
///
/// - `message`: Static detail explaining the rejected configuration shape.
///
/// # Returns
///
/// An invalid-configuration failure carrying an `InvalidOptions` filesystem
/// error.
#[inline(always)]
fn invalid_options(message: &'static str) -> ProviderFailure<FsError> {
    ProviderFailure::invalid_configuration(FsError::new(
        FsErrorKind::InvalidOptions,
        FsOperation::Provider,
        message,
    ))
}

/// Builds an invalid-path provider failure.
///
/// # Parameters
///
/// - `message`: Static detail explaining why the path is invalid.
///
/// # Returns
///
/// An invalid-configuration failure carrying an `InvalidPath` filesystem
/// error.
#[inline(always)]
pub(super) fn invalid_path(message: &'static str) -> ProviderFailure<FsError> {
    ProviderFailure::invalid_configuration(FsError::new(
        FsErrorKind::InvalidPath,
        FsOperation::Provider,
        message,
    ))
}
