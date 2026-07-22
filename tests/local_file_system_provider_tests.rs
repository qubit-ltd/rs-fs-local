// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_fs::{
    CredentialRef,
    FileKind,
    FileSystemConfig,
    FileSystemRegistry,
    FileSystemSpec,
    FsUri,
};
use qubit_fs_local::LocalFileSystemProvider;
use qubit_metadata::Metadata;
use qubit_spi::{
    ProviderMetadata,
    ServiceProvider,
    error::ProviderErrorKind,
};

/// Asserts that the local provider rejects one unsupported configuration.
///
/// # Parameters
///
/// * `config` - Configuration expected to be unsupported.
///
/// # Panics
///
/// Panics when the provider accepts the configuration or reports a different
/// error kind.
fn assert_invalid_configuration(config: FileSystemConfig) {
    let result = ServiceProvider::<FileSystemSpec>::create_configured(
        &LocalFileSystemProvider,
        &config,
    );
    let error = match result {
        Ok(_) => panic!("unsupported local configuration should fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), ProviderErrorKind::InvalidConfiguration);
}

/// Confirms the provider exposes its canonical id and `file` selector alias.
#[test]
fn test_provider_descriptor_exposes_file_alias() {
    let descriptor = LocalFileSystemProvider.descriptor();

    assert_eq!(descriptor.id().as_str(), "local-file");
    assert_eq!(descriptor.aliases().len(), 1);
    assert_eq!(descriptor.aliases()[0].as_str(), "file");
}

/// Confirms the provider's standard value traits remain usable.
#[test]
fn test_provider_value_traits() {
    let provider = LocalFileSystemProvider;
    let cloned = provider.clone();

    assert_eq!(format!("{cloned:?}"), "LocalFileSystemProvider");
}

/// Confirms registry resolution decodes URI percent escapes exactly once.
#[cfg(unix)]
#[test]
fn test_registry_resolves_percent_encoded_file_uri() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("100%ready.txt");
    std::fs::write(&file_path, b"payload").expect("write test file");
    let encoded_path = file_path
        .to_str()
        .expect("temporary path should be UTF-8")
        .replace('%', "%25");
    let uri = FsUri::parse(&format!("file://{encoded_path}"))
        .expect("parse file URI");
    let config = FileSystemConfig::new(uri);
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider)
        .expect("register local provider");

    let resource = registry.resource(&config).expect("resolve local resource");
    let metadata = resource.stat().expect("stat resolved resource");

    assert_eq!(metadata.kind, FileKind::File);
    assert_eq!(metadata.len, Some(7));
}

/// Confirms alphabetic hexadecimal digits in URI escapes are decoded.
#[cfg(unix)]
#[test]
fn test_registry_decodes_alphabetic_hexadecimal_uri_escape() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("ready?.txt");
    std::fs::write(&file_path, b"payload").expect("write test file");
    let encoded_path = file_path
        .to_str()
        .expect("temporary path should be UTF-8")
        .replace('?', "%3F");
    let uri = FsUri::parse(&format!("file://{encoded_path}"))
        .expect("parse file URI");
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider)
        .expect("register local provider");

    let resource = registry.resource_uri(&uri).expect("resolve local resource");

    assert_eq!(resource.stat().expect("stat resource").kind, FileKind::File);
}

/// Confirms Windows drive paths round-trip through a percent-encoded file URI.
#[cfg(windows)]
#[test]
fn test_registry_reads_percent_encoded_windows_file_uri() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("100%ready.txt");
    std::fs::write(&file_path, b"payload").expect("write test file");
    let encoded_path = file_path
        .to_str()
        .expect("temporary path should be UTF-8")
        .replace('\\', "/")
        .replace('%', "%25");
    let uri = FsUri::parse(&format!("file:///{encoded_path}"))
        .expect("parse Windows file URI");
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider)
        .expect("register local provider");

    let resource = registry.resource_uri(&uri).expect("resolve local resource");

    assert!(resource.path().is_absolute());
    assert_eq!(resource.read_all(64).expect("read resource"), b"payload");
}

/// Confirms alphabetic hexadecimal URI escapes work in Windows drive paths.
#[cfg(windows)]
#[test]
fn test_registry_decodes_alphabetic_windows_uri_escape() {
    let temporary_directory =
        tempfile::tempdir().expect("create temporary directory");
    let file_path = temporary_directory.path().join("readyA.txt");
    std::fs::write(&file_path, b"payload").expect("write test file");
    let encoded_path = file_path
        .to_str()
        .expect("temporary path should be UTF-8")
        .replace('\\', "/")
        .replace('A', "%41");
    let uri = FsUri::parse(&format!("file:///{encoded_path}"))
        .expect("parse Windows file URI");
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider)
        .expect("register local provider");

    let resource = registry.resource_uri(&uri).expect("resolve local resource");

    assert_eq!(resource.stat().expect("stat resource").kind, FileKind::File);
}

/// Confirms the provider rejects schemes other than `file`.
#[test]
fn test_provider_rejects_non_file_scheme() {
    let uri = FsUri::parse("memory:///tmp/item.txt").expect("parse URI");

    assert_invalid_configuration(FileSystemConfig::new(uri));
}

/// Confirms the provider rejects remote file authorities.
#[test]
fn test_provider_rejects_non_empty_authority() {
    let uri = FsUri::parse("file://remote/tmp/item.txt").expect("parse URI");

    assert_invalid_configuration(FileSystemConfig::new(uri));
}

/// Confirms the provider rejects URI queries.
#[test]
fn test_provider_rejects_query() {
    let uri =
        FsUri::parse("file:///tmp/item.txt?version=1").expect("parse URI");

    assert_invalid_configuration(FileSystemConfig::new(uri));
}

/// Confirms the provider rejects provider-specific options.
#[test]
fn test_provider_rejects_options() {
    let uri = FsUri::parse("file:///tmp/item.txt").expect("parse URI");
    let options = Metadata::new().with("mode", "readonly");
    let config = FileSystemConfig::new(uri)
        .with_options(options)
        .expect("set non-sensitive options");

    assert_invalid_configuration(config);
}

/// Confirms the provider rejects credential references.
#[test]
fn test_provider_rejects_credentials() {
    let uri = FsUri::parse("file:///tmp/item.txt").expect("parse URI");
    let config = FileSystemConfig::new(uri)
        .with_credentials(CredentialRef::DefaultChain);

    assert_invalid_configuration(config);
}

/// Confirms the provider rejects relative native paths.
#[test]
fn test_provider_rejects_relative_path() {
    let uri = FsUri::parse("file:relative/item.txt").expect("parse URI");

    assert_invalid_configuration(FileSystemConfig::new(uri));
}

/// Confirms canonical path validation rejects traversal above the root.
#[test]
fn test_provider_rejects_path_above_root() {
    let uri = FsUri::parse("file:///../item.txt").expect("parse URI");

    assert_invalid_configuration(FileSystemConfig::new(uri));
}
