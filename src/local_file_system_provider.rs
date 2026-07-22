// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Defines the service-provider adapter for the local filesystem.

use std::{
    ffi::OsStr,
    path::Path,
    sync::Arc,
};

use qubit_fs::{
    FileSystem,
    FileSystemConfig,
    FileSystemResolution,
    FileSystemSpec,
    FsPath,
    FsUriPath,
    NativePathCodec,
    OsStrPathCodec,
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
        if !config.options().is_empty() {
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
    /// native-absolute or violates canonical filesystem path semantics.
    ///
    /// # Panics
    ///
    /// Panics only if validated [`FsUriPath`] text violates its percent-escape,
    /// UTF-8, or native path codec invariants.
    fn decode_path(path: &FsUriPath) -> Result<FsPath, ProviderError> {
        let decoded = percent_decode(path.as_encoded());
        let native_text = native_uri_path(&decoded);
        let native_path = Path::new(native_text);
        if !native_path.is_absolute() {
            return Err(ProviderError::invalid_configuration(
                "local file URI path must be absolute",
            ));
        }
        let canonical = OsStrPathCodec
            .decode(OsStr::new(native_text))
            .expect("validated UTF-8 URI path must be a valid native path");
        FsPath::parse(canonical.as_ref()).map_err(|error| {
            ProviderError::invalid_configuration_with_source(
                "local file URI path is not a valid filesystem path",
                error,
            )
        })
    }
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
        ProviderDescriptor::new(LocalFileSystem::provider_id())
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

/// Decodes percent escapes in one URI path without applying path semantics.
///
/// # Parameters
///
/// * `encoded` - Validated URI path text.
///
/// # Returns
///
/// The UTF-8 path text represented by the URI bytes.
///
/// # Panics
///
/// Panics when `encoded` contains an incomplete or non-hexadecimal escape, or
/// when its decoded bytes are not valid UTF-8. Callers pass a validated
/// [`FsUriPath`], whose invariants exclude these cases.
fn percent_decode(encoded: &str) -> String {
    let input = encoded.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] != b'%' {
            output.push(input[index]);
            index += 1;
            continue;
        }
        let digits = &encoded[index + 1..index + 3];
        let byte = u8::from_str_radix(digits, 16)
            .expect("FsUriPath must contain complete hexadecimal escapes");
        output.push(byte);
        index += 3;
    }
    String::from_utf8(output)
        .expect("FsUriPath must contain percent-encoded UTF-8 text")
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
