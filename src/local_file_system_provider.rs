// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Defines the service-provider adapter for the local filesystem.

use std::{
    path::{
        Component,
        Path,
    },
    sync::Arc,
};

use qubit_fs::{
    FileSystem,
    FsPath,
    FsUriPath,
};
use qubit_fs_registry::{
    FileSystemConfig,
    FileSystemResolution,
    FileSystemSpec,
};
use qubit_spi::{
    ProviderDescriptor,
    ProviderMetadata,
    ServiceProvider,
    error::ProviderError,
};

use crate::LocalFileSystem;

/// Creates synchronous host-local filesystems for `file:` URIs.
#[derive(Clone, Debug, Default)]
pub struct LocalFileSystemProvider;

impl LocalFileSystemProvider {
    /// Validates provider-specific configuration accepted by the local backend.
    ///
    /// # Parameters
    ///
    /// * `config` - Filesystem configuration supplied by the registry.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the configuration contains only supported local fields.
    ///
    /// # Errors
    ///
    /// Returns an invalid-configuration error for a non-`file` scheme, remote
    /// authority, query, provider options, or credentials.
    fn validate_config(config: &FileSystemConfig) -> Result<(), ProviderError> {
        if config.uri().scheme().as_str() != "file" {
            return Err(ProviderError::invalid_configuration(
                "local filesystem provider requires the file URI scheme",
            ));
        }
        if config.uri().authority().is_some() {
            return Err(ProviderError::invalid_configuration(
                "local filesystem provider does not support URI authorities",
            ));
        }
        if !config.uri().query().is_empty() {
            return Err(ProviderError::invalid_configuration(
                "local filesystem provider does not support URI queries",
            ));
        }
        if config.options().iter().next().is_some() {
            return Err(ProviderError::invalid_configuration(
                "local filesystem provider does not support provider options",
            ));
        }
        if config.credentials().is_some() {
            return Err(ProviderError::invalid_configuration(
                "local filesystem provider does not support credentials",
            ));
        }
        Ok(())
    }

    /// Converts an encoded file-URI path into a canonical filesystem path.
    ///
    /// # Parameters
    ///
    /// * `path` - Validated URI path containing percent escapes.
    ///
    /// # Returns
    ///
    /// A canonical path preserving native path code units without loss.
    ///
    /// # Errors
    ///
    /// Returns an invalid-configuration error when the decoded path is not
    /// native-absolute, a URI component introduces a native boundary, or the
    /// resulting native path violates canonical filesystem path semantics.
    ///
    /// # Panics
    ///
    /// Panics only if validated URI text violates the native path codec
    /// invariant.
    fn decode_path(path: &FsUriPath) -> Result<FsPath, ProviderError> {
        let decoded = decode_uri_path_components(path)?;
        let native_text = native_uri_path(&decoded);
        let native_path = Path::new(native_text);
        if !native_path.is_absolute() {
            return Err(ProviderError::invalid_configuration(
                "local file URI path must be absolute",
            ));
        }
        LocalFileSystem::path_from_native(native_path).map_err(|error| {
            ProviderError::invalid_configuration_with_source(
                "local file URI path is not a valid filesystem path",
                error,
            )
        })
    }
}

/// Decodes one file-URI path while retaining its literal component boundaries.
///
/// URI percent decoding happens separately for each component. A decoded
/// component must remain exactly one normal native component, except for the
/// leading Windows drive component required by file URI syntax.
///
/// # Parameters
///
/// * `path` - Validated encoded URI path.
///
/// # Returns
///
/// Decoded slash-separated URI path text suitable for platform adaptation.
///
/// # Errors
///
/// Returns an invalid-configuration error when the URI path is relative or a
/// decoded component introduces a native separator, root, prefix, or dot
/// component.
fn decode_uri_path_components(
    path: &FsUriPath,
) -> Result<String, ProviderError> {
    let encoded = path.as_encoded();
    if !encoded.starts_with('/') {
        return Err(ProviderError::invalid_configuration(
            "local file URI path must be absolute",
        ));
    }
    let mut decoded_components = Vec::new();
    for (index, encoded_component) in encoded
        .split('/')
        .filter(|component| !component.is_empty())
        .enumerate()
    {
        let decoded_component = FsUriPath::parse(encoded_component)
            .map_err(|error| {
                ProviderError::invalid_configuration_with_source(
                    "local file URI path contains an invalid component",
                    error,
                )
            })?
            .decode();
        if !is_native_uri_component(&decoded_component, index == 0) {
            return Err(ProviderError::invalid_configuration(
                "local file URI component introduces a native path boundary",
            ));
        }
        decoded_components.push(decoded_component);
    }
    Ok(format!("/{}", decoded_components.join("/")))
}

/// Reports whether one decoded URI component remains one native component.
///
/// # Parameters
///
/// * `component` - URI-decoded component text.
/// * `is_first` - Whether this is the first component after the URI root.
///
/// # Returns
///
/// `true` when the component is a single native normal component, or the
/// platform-specific leading drive component permitted by file URIs.
#[must_use]
fn is_native_uri_component(component: &str, is_first: bool) -> bool {
    #[cfg(windows)]
    if is_first && is_windows_drive_component(component) {
        return true;
    }
    #[cfg(not(windows))]
    let _ = is_first;
    let mut components = Path::new(component).components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
}

/// Reports whether a URI-decoded component names a Windows drive prefix.
///
/// # Parameters
///
/// * `component` - First URI-decoded component after the leading slash.
///
/// # Returns
///
/// `true` only for an ASCII drive letter followed by a colon.
#[cfg(windows)]
#[must_use]
fn is_windows_drive_component(component: &str) -> bool {
    let bytes = component.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

impl ProviderMetadata for LocalFileSystemProvider {
    /// Returns the canonical provider descriptor and its `file` alias.
    ///
    /// # Returns
    ///
    /// The validated `local-file` descriptor with the `file` alias.
    ///
    /// # Panics
    ///
    /// Panics only if a static provider id or alias violates SPI grammar.
    #[inline]
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            qubit_spi::ProviderId::new(LocalFileSystem::provider_id())
                .expect("the static local provider ID must be valid"),
        )
        .with_aliases(["file"])
        .expect("static local provider alias must be valid")
    }
}

impl ServiceProvider<FileSystemSpec> for LocalFileSystemProvider {
    /// Creates a host-local filesystem resolution for one `file:` URI.
    ///
    /// # Parameters
    ///
    /// * `config` - Registry configuration to validate and resolve.
    ///
    /// # Returns
    ///
    /// A synchronous local filesystem, canonical path, and original URI.
    ///
    /// # Errors
    ///
    /// Returns an invalid-configuration error for unsupported fields or an
    /// invalid local URI path.
    ///
    /// # Panics
    ///
    /// Panics only if the provider's static filesystem id, provider id, URI
    /// scheme, or alias violates its corresponding grammar.
    #[inline]
    fn create_configured(
        &self,
        config: &FileSystemConfig,
    ) -> Result<FileSystemResolution<dyn FileSystem>, ProviderError> {
        Self::validate_config(config)?;
        let path = Self::decode_path(config.uri().path())?;
        let fs: Arc<dyn FileSystem> = Arc::new(LocalFileSystem::host());
        Ok(FileSystemResolution::new(fs, path, config.uri().clone()))
    }
}

/// Adapts decoded file-URI text to the current platform's native path form.
///
/// # Parameters
///
/// * `decoded` - Percent-decoded file-URI path text.
///
/// # Returns
///
/// A borrowed native path spelling for the current platform.
#[cfg(not(windows))]
#[inline(always)]
fn native_uri_path(decoded: &str) -> &str {
    decoded
}

/// Adapts `/C:/...` file-URI paths to Windows drive-path spelling.
///
/// # Parameters
///
/// * `decoded` - Percent-decoded file-URI path text.
///
/// # Returns
///
/// A borrowed Windows path with the URI-only leading slash removed for drive
/// paths.
#[cfg(windows)]
#[inline]
fn native_uri_path(decoded: &str) -> &str {
    let bytes = decoded.as_bytes();
    if bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
    {
        &decoded[1..]
    } else {
        decoded
    }
}
