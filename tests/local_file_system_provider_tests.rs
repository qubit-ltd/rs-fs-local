// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Registry integration tests for the local `file:` provider.

use std::sync::Arc;
use std::sync::Mutex;

use qubit_fs::FileSystem;
use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::metadata::NonSensitiveMetadata;
use qubit_fs::metadata::UserMetadata;
use qubit_fs::path::ConnectionUri;
use qubit_fs::path::Path;
use qubit_fs::spi::FileSystemSpi;
use qubit_fs_local::LocalFileSystemProvider;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::spi::LocalFileSystemSpi;
use qubit_fs_registry::CredentialRef;
use qubit_fs_registry::FileSystemConfig;
use qubit_fs_registry::FileSystemRegistry;
use qubit_fs_registry::FileSystemRegistryError;
use qubit_fs_registry::FileSystemResolution;
use qubit_fs_registry::FileSystemSpec;
use qubit_spi::FallbackPolicy;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;
use qubit_spi::ProviderMetadata;
use qubit_spi::ProviderSelection;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;
use qubit_spi::error::ProviderFailureKind;

/// Repeated SPI property queries return the same validated provider snapshot.
#[test]
fn test_local_spi_returns_a_stable_validated_provider_snapshot() {
    let spi = LocalFileSystemSpi::new(LocalResourcePolicy::unbounded())
        .expect("local SPI should construct");
    let first = FileSystemSpi::properties(&spi);
    let second = FileSystemSpi::properties(&spi);

    assert_eq!(first.info(), second.info());
    assert_eq!(first.operations(), second.operations());
    assert_eq!(
        first.declared_capabilities(),
        second.declared_capabilities()
    );
    assert_eq!(first.limits(), second.limits());
    assert_eq!(first.path_constraints(), second.path_constraints());
    assert_eq!(first.symlink_policy(), second.symlink_policy());
}

/// A registered host provider resolves an absolute `file:` URI.
#[test]
fn test_local_provider_returns_concrete_resolution() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("the local provider descriptor must register");
    let config = FileSystemConfig::new(
        ConnectionUri::parse("file:///tmp/data").expect("the test file URI must parse"),
    );

    let resolution = registry
        .resolve_config(&config)
        .expect("the local provider must resolve a host file URI");

    let _: &FileSystem = resolution.file_system();
    assert_eq!(resolution.canonical_uri().scheme(), "file");
}

/// Equivalent absolute `file:` spellings resolve to one canonical URI.
#[test]
fn test_local_provider_canonicalizes_file_uri_path() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("the local provider descriptor must register");

    let single_slash = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:/tmp/data").expect("single-slash URI must parse"),
        ))
        .expect("single-slash file URI must resolve");
    let triple_slash = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///tmp/data").expect("triple-slash URI must parse"),
        ))
        .expect("triple-slash file URI must resolve");

    assert_eq!(single_slash.canonical_uri(), triple_slash.canonical_uri());
    assert_eq!(single_slash.canonical_uri().as_str(), "file:///tmp/data");
}

/// Remote URI authorities do not have host-local filesystem authority.
#[test]
fn test_local_provider_rejects_remote_authority() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("the local provider descriptor must register");
    let config = FileSystemConfig::new(
        ConnectionUri::parse("file://remote/share").expect("the test URI must parse"),
    );

    let error = registry
        .resolve_config(&config)
        .expect_err("remote file authority must be rejected");
    let FileSystemRegistryError::Creation(creation) = error else {
        panic!("expected provider creation error")
    };
    assert_eq!(
        creation.decisive_attempt().failure().error().kind(),
        FsErrorKind::InvalidOptions
    );
}

/// Provider configuration rejects every URI and metadata feature outside the
/// local adapter contract.
#[test]
fn test_local_provider_rejects_unsupported_configuration_shapes() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("the local provider descriptor must register");
    let unsupported_scheme =
        FileSystemConfig::new(ConnectionUri::parse("memory:///data").expect("test URI must parse"));
    assert!(matches!(
        registry.resolve_config(&unsupported_scheme),
        Err(FileSystemRegistryError::Resolution(_))
    ));

    for config in [
        FileSystemConfig::new(
            ConnectionUri::parse("file:///data?cache=true").expect("test URI must parse"),
        ),
        FileSystemConfig::new(ConnectionUri::parse("file:///data").expect("test URI must parse"))
            .with_options(NonSensitiveMetadata::from(
                UserMetadata::new()
                    .with("mode", "test")
                    .expect("test metadata must be valid"),
            )),
    ] {
        let error = registry
            .resolve_config(&config)
            .expect_err("unsupported provider configuration must be rejected");
        let FileSystemRegistryError::Creation(creation) = error else {
            panic!("expected provider creation error")
        };
        assert_eq!(
            creation.decisive_attempt().failure().error().kind(),
            FsErrorKind::InvalidOptions
        );
    }

    let relative = FileSystemConfig::new(
        ConnectionUri::parse("file:relative/path").expect("relative file URI must parse"),
    );
    let error = registry
        .resolve_config(&relative)
        .expect_err("relative local file URIs must be rejected");
    let FileSystemRegistryError::Creation(creation) = error else {
        panic!("expected provider creation error")
    };
    assert_eq!(
        creation.decisive_attempt().failure().error().kind(),
        FsErrorKind::InvalidPath
    );
}

/// A named local selection makes one unsupported attempt for a non-`file:` URI.
#[test]
fn test_named_local_provider_rejects_non_file_uri_without_fallback() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("the local provider descriptor must register");
    let config =
        FileSystemConfig::new(ConnectionUri::parse("memory:///data").expect("test URI must parse"))
            .with_selection(
                ProviderSelection::named("local-file").expect("local selection must parse"),
            );

    let error = registry
        .resolve_config(&config)
        .expect_err("a named local selection must reject a non-file URI");
    let FileSystemRegistryError::Creation(creation) = error else {
        panic!("expected provider creation error")
    };

    assert_eq!(creation.attempts().len(), 1);
    assert_eq!(creation.attempts()[0].provider_id().as_str(), "local-file");
    assert_eq!(
        creation.decisive_attempt().failure().kind(),
        ProviderFailureKind::Unsupported,
    );
    assert_eq!(
        creation.decisive_attempt().failure().error().kind(),
        FsErrorKind::UnsupportedOperation,
    );
}

/// Automatic selection tries the priority-zero local provider before a
/// priority-negative remote candidate and records both attempts.
#[test]
fn test_auto_selection_orders_local_before_lower_priority_remote() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("the local provider descriptor must register");
    registry
        .register(AlwaysUnsupportedProvider {
            descriptor: ProviderDescriptor::new(
                ProviderId::new("remote").expect("provider ID must be valid"),
            )
            .with_priority(-1),
        })
        .expect("the remote provider descriptor must register");
    let config =
        FileSystemConfig::new(ConnectionUri::parse("memory:///data").expect("test URI must parse"))
            .with_selection(ProviderSelection::auto());

    let error = registry
        .resolve_config(&config)
        .expect_err("both test providers must reject the memory URI");
    let FileSystemRegistryError::Creation(creation) = error else {
        panic!("expected provider creation error")
    };

    assert_eq!(creation.attempts().len(), 2);
    assert_eq!(
        creation
            .attempts()
            .iter()
            .map(|attempt| attempt.provider_id().as_str())
            .collect::<Vec<_>>(),
        ["local-file", "remote"],
    );
}

/// An unsupported local scheme permits a chained provider to resolve the URI.
#[test]
fn test_local_provider_chain_falls_back_for_unsupported_scheme() {
    let local = LocalFileSystemProvider::host(LocalResourcePolicy::unbounded());
    let direct_config =
        FileSystemConfig::new(ConnectionUri::parse("memory:///data").expect("test URI must parse"))
            .with_options(NonSensitiveMetadata::from(
                UserMetadata::new()
                    .with("mode", "test")
                    .expect("test metadata must be valid"),
            ));
    let direct_failure = local
        .create_configured(&direct_config)
        .expect_err("local provider must reject unsupported schemes");
    assert_eq!(direct_failure.kind(), ProviderFailureKind::Unsupported);
    assert_eq!(
        direct_failure.error().kind(),
        FsErrorKind::UnsupportedOperation
    );

    let registry = FileSystemRegistry::default();
    registry
        .register(local)
        .expect("the local provider descriptor must register");
    let root = tempfile::tempdir().expect("fallback provider root must be created");
    let fallback = LocalFileSystemProvider::rooted_with_descriptor(
        ProviderDescriptor::new(
            ProviderId::new("chain-fallback").expect("provider id must be valid"),
        ),
        FileSystemId::new("chain-fallback-root").expect("filesystem id must be valid"),
        root.path(),
        LocalResourcePolicy::unbounded(),
    )
    .expect("the fallback local provider must open");
    let received_schemes = Arc::new(Mutex::new(Vec::new()));
    registry
        .register(ChainFallbackProvider {
            inner: fallback,
            received_schemes: Arc::clone(&received_schemes),
        })
        .expect("the chain fallback provider must register");
    let selection = ProviderSelection::chain(["local-file", "chain-fallback"])
        .expect("provider chain must parse")
        .with_fallback_policy(FallbackPolicy::OnAbsence);

    for uri in [
        "memory:///data",
        "memory://user:password@localhost/data",
        "memory:///data?token=secret",
    ] {
        let config = FileSystemConfig::new(ConnectionUri::parse(uri).expect("test URI must parse"))
            .with_selection(selection.clone());

        let resolution = registry
            .resolve_config(&config)
            .expect("unsupported local scheme must fall back to the next provider");

        assert_eq!(
            resolution.path(),
            &Path::parse("/fallback").expect("path must parse")
        );
        assert_eq!(resolution.canonical_uri().as_str(), "file:///fallback");
    }

    let credential_config =
        FileSystemConfig::new(ConnectionUri::parse("memory:///data").expect("test URI must parse"))
            .with_credential(CredentialRef::DefaultChain)
            .with_selection(selection);
    let resolution = registry
        .resolve_config(&credential_config)
        .expect("unsupported scheme must precede external credential handling");
    assert_eq!(resolution.canonical_uri().as_str(), "file:///fallback");

    assert_eq!(
        received_schemes
            .lock()
            .expect("the fixture scheme log must not be poisoned")
            .as_slice(),
        ["memory", "memory", "memory", "memory"],
    );
}

/// A `file:` configuration error stops an absence-only chain before the next
/// provider is invoked.
#[test]
fn test_local_provider_chain_does_not_fallback_for_file_configuration_error() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("the local provider descriptor must register");
    let root = tempfile::tempdir().expect("fallback provider root must be created");
    let fallback = LocalFileSystemProvider::rooted_with_descriptor(
        ProviderDescriptor::new(
            ProviderId::new("chain-fallback").expect("provider id must be valid"),
        ),
        FileSystemId::new("chain-fallback-root").expect("filesystem id must be valid"),
        root.path(),
        LocalResourcePolicy::unbounded(),
    )
    .expect("the fallback local provider must open");
    let received_schemes = Arc::new(Mutex::new(Vec::new()));
    registry
        .register(ChainFallbackProvider {
            inner: fallback,
            received_schemes: Arc::clone(&received_schemes),
        })
        .expect("the chain fallback provider must register");

    let config = FileSystemConfig::new(
        ConnectionUri::parse("file:///data?cache=true").expect("test URI must parse"),
    )
    .with_selection(
        ProviderSelection::chain(["local-file", "chain-fallback"])
            .expect("provider chain must parse")
            .with_fallback_policy(FallbackPolicy::OnAbsence),
    );
    let error = registry
        .resolve_config(&config)
        .expect_err("file configuration errors must be terminal in an absence-only chain");
    let FileSystemRegistryError::Creation(creation) = error else {
        panic!("expected provider creation error")
    };

    assert_eq!(creation.attempts().len(), 1);
    assert_eq!(creation.attempts()[0].provider_id().as_str(), "local-file");
    assert_eq!(
        creation.decisive_attempt().failure().error().kind(),
        FsErrorKind::InvalidOptions,
    );
    assert!(
        received_schemes
            .lock()
            .expect("the fixture scheme log must not be poisoned")
            .is_empty(),
        "the second provider must not be invoked",
    );
}

/// An embedded credential makes a `file:` configuration terminal in an
/// absence-only chain before the next provider is invoked.
#[test]
fn test_local_provider_chain_does_not_fallback_for_embedded_file_credential() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("the local provider descriptor must register");
    let root = tempfile::tempdir().expect("fallback provider root must be created");
    let fallback = LocalFileSystemProvider::rooted_with_descriptor(
        ProviderDescriptor::new(
            ProviderId::new("chain-fallback").expect("provider id must be valid"),
        ),
        FileSystemId::new("chain-fallback-root").expect("filesystem id must be valid"),
        root.path(),
        LocalResourcePolicy::unbounded(),
    )
    .expect("the fallback local provider must open");
    let received_schemes = Arc::new(Mutex::new(Vec::new()));
    registry
        .register(ChainFallbackProvider {
            inner: fallback,
            received_schemes: Arc::clone(&received_schemes),
        })
        .expect("the chain fallback provider must register");

    let config = FileSystemConfig::new(
        ConnectionUri::parse("file://user@localhost/data").expect("test URI must parse"),
    )
    .with_selection(
        ProviderSelection::chain(["local-file", "chain-fallback"])
            .expect("provider chain must parse")
            .with_fallback_policy(FallbackPolicy::OnAbsence),
    );
    let error = registry
        .resolve_config(&config)
        .expect_err("embedded file credentials must be terminal in an absence-only chain");
    let FileSystemRegistryError::Creation(creation) = error else {
        panic!("expected provider creation error")
    };

    assert_eq!(creation.attempts().len(), 1);
    assert_eq!(
        creation.decisive_attempt().failure().error().kind(),
        FsErrorKind::InvalidOptions,
    );
    assert!(
        received_schemes
            .lock()
            .expect("the fixture scheme log must not be poisoned")
            .is_empty(),
        "the second provider must not be invoked",
    );
}

/// An external credential reference makes a `file:` configuration terminal in
/// an absence-only chain before the next provider is invoked.
#[test]
fn test_local_provider_chain_does_not_fallback_for_referenced_file_credential() {
    let registry = FileSystemRegistry::default();
    registry
        .register(LocalFileSystemProvider::host(
            LocalResourcePolicy::unbounded(),
        ))
        .expect("the local provider descriptor must register");
    let root = tempfile::tempdir().expect("fallback provider root must be created");
    let fallback = LocalFileSystemProvider::rooted_with_descriptor(
        ProviderDescriptor::new(
            ProviderId::new("chain-fallback").expect("provider id must be valid"),
        ),
        FileSystemId::new("chain-fallback-root").expect("filesystem id must be valid"),
        root.path(),
        LocalResourcePolicy::unbounded(),
    )
    .expect("the fallback local provider must open");
    let received_schemes = Arc::new(Mutex::new(Vec::new()));
    registry
        .register(ChainFallbackProvider {
            inner: fallback,
            received_schemes: Arc::clone(&received_schemes),
        })
        .expect("the chain fallback provider must register");

    let config =
        FileSystemConfig::new(ConnectionUri::parse("file:///data").expect("test URI must parse"))
            .with_credential(CredentialRef::DefaultChain)
            .with_selection(
                ProviderSelection::chain(["local-file", "chain-fallback"])
                    .expect("provider chain must parse")
                    .with_fallback_policy(FallbackPolicy::OnAbsence),
            );
    let error = registry
        .resolve_config(&config)
        .expect_err("referenced file credentials must be terminal in an absence-only chain");
    let FileSystemRegistryError::Creation(creation) = error else {
        panic!("expected provider creation error")
    };

    assert_eq!(creation.attempts().len(), 1);
    assert_eq!(creation.attempts()[0].provider_id().as_str(), "local-file");
    assert_eq!(
        creation.decisive_attempt().failure().error().kind(),
        FsErrorKind::InvalidOptions,
    );
    assert!(
        received_schemes
            .lock()
            .expect("the fixture scheme log must not be poisoned")
            .is_empty(),
        "the second provider must not be invoked",
    );
}

/// Embedded secrets are unsupported by the local provider and must return a
/// provider-creation error instead of panicking while decoding the URI.
#[test]
fn test_local_provider_rejects_embedded_secrets_without_panicking() {
    for text in [
        "file://user:password@localhost/tmp/data",
        "file:///tmp/data?token=secret",
    ] {
        let registry = FileSystemRegistry::default();
        registry
            .register(LocalFileSystemProvider::host(
                LocalResourcePolicy::unbounded(),
            ))
            .expect("the local provider descriptor must register");
        let config = FileSystemConfig::new(
            ConnectionUri::parse(text).expect("test connection URI must parse"),
        );

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.resolve_config(&config)
        }));

        let result = outcome.expect("embedded secrets must not panic");
        assert!(matches!(result, Err(FileSystemRegistryError::Creation(_))));
    }
}

/// A rooted provider resolves accepted file URIs inside its retained native
/// authority.
#[test]
fn test_rooted_local_provider_resolves_file_uri() {
    let root = tempfile::tempdir().expect("provider root must be created");
    let id = FileSystemId::new("provider-rooted-local").expect("test identity must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted(
                id.clone(),
                root.path(),
                LocalResourcePolicy::unbounded(),
            )
            .expect("rooted provider must open"),
        )
        .expect("the rooted local provider descriptor must register");
    let config = FileSystemConfig::new(
        ConnectionUri::parse("file:///inside-root").expect("test URI must parse"),
    );

    let resolution = registry
        .resolve_config(&config)
        .expect("rooted local provider must resolve file URI");

    assert_eq!(resolution.file_system().properties().info().id(), &id);
}

/// A rooted provider retains the authority opened at construction even when
/// the diagnostic root pathname is later replaced.
#[test]
fn test_rooted_local_provider_pins_opened_authority() {
    let parent = tempfile::tempdir().expect("provider parent must be created");
    let root = parent.path().join("root");
    std::fs::create_dir(&root).expect("provider root must be created");
    std::fs::write(root.join("value"), b"original").expect("fixture must be written");
    let id = FileSystemId::new("provider-pinned-root").expect("test identity must be valid");
    let provider = LocalFileSystemProvider::rooted(id, &root, LocalResourcePolicy::unbounded())
        .expect("rooted provider must open");
    std::fs::rename(&root, parent.path().join("old-root"))
        .expect("opened root path must be replaceable");
    std::fs::create_dir(&root).expect("replacement root must be created");
    std::fs::write(root.join("value"), b"replacement")
        .expect("replacement fixture must be written");

    let registry = FileSystemRegistry::default();
    registry.register(provider).expect("provider must register");
    let resolution = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///value").expect("test URI must parse"),
        ))
        .expect("provider must resolve pinned authority");
    assert_eq!(
        b"original".to_vec(),
        resolution
            .file_system()
            .read_all(resolution.path(), Default::default(), 1024)
            .expect("pinned root must retain original entry")
    );
}

/// A rooted provider decodes percent-encoded file URI path segments before
/// accessing its retained native authority.
#[test]
fn test_rooted_local_provider_decodes_percent_encoded_path_segments() {
    let root = tempfile::tempdir().expect("provider root must be created");
    std::fs::write(root.path().join("report final.txt"), b"payload")
        .expect("encoded-path fixture must be written");
    std::fs::write(root.path().join("café.txt"), b"payload")
        .expect("UTF-8 encoded-path fixture must be written");
    let id = FileSystemId::new("provider-encoded-path-root").expect("test identity must be valid");
    let registry = FileSystemRegistry::default();
    registry
        .register(
            LocalFileSystemProvider::rooted(id, root.path(), LocalResourcePolicy::unbounded())
                .expect("rooted provider must open"),
        )
        .expect("the rooted local provider descriptor must register");

    for uri in ["file:///report%20final.txt", "file:///caf%C3%A9.txt"] {
        let resolution = registry
            .resolve_config(&FileSystemConfig::new(
                ConnectionUri::parse(uri).expect("test URI must parse"),
            ))
            .expect("encoded local file URI must resolve");
        resolution
            .file_system()
            .stat(resolution.path())
            .expect("decoded path must access the native fixture");
    }

    assert_eq!(
        Path::parse("/report final.txt").expect("test logical path must parse"),
        registry
            .resolve_config(&FileSystemConfig::new(
                ConnectionUri::parse("file:///report%20final.txt").expect("test URI must parse"),
            ))
            .expect("encoded local file URI must resolve")
            .path()
            .clone()
    );
}

/// Rooted providers reject a native authority that cannot be opened.
#[test]
fn test_rooted_local_provider_rejects_missing_root() {
    let root = std::env::temp_dir().join(format!(
        "qubit-fs-local-missing-root-{}",
        std::process::id()
    ));
    let id = FileSystemId::new("provider-missing-root").expect("test identity must be valid");
    assert!(
        LocalFileSystemProvider::rooted(id, &root, LocalResourcePolicy::unbounded()).is_err(),
        "a missing rooted authority must be rejected"
    );
}

struct ChainFallbackProvider {
    inner: LocalFileSystemProvider,
    received_schemes: Arc<Mutex<Vec<String>>>,
}

struct AlwaysUnsupportedProvider {
    descriptor: ProviderDescriptor,
}

impl ProviderMetadata for AlwaysUnsupportedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }
}

impl ServiceProvider<FileSystemSpec> for AlwaysUnsupportedProvider {
    fn create_configured(
        &self,
        _: &FileSystemConfig,
    ) -> Result<FileSystemResolution, ProviderFailure<FsError>> {
        Err(ProviderFailure::unsupported(FsError::new(
            FsErrorKind::UnsupportedOperation,
            FsOperation::Provider,
            "test provider does not support this URI",
        )))
    }
}

impl ProviderMetadata for ChainFallbackProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.inner.descriptor()
    }
}

impl ServiceProvider<FileSystemSpec> for ChainFallbackProvider {
    fn create_configured(
        &self,
        config: &FileSystemConfig,
    ) -> Result<FileSystemResolution, ProviderFailure<FsError>> {
        self.received_schemes
            .lock()
            .expect("the fixture scheme log must not be poisoned")
            .push(config.uri().scheme().to_owned());
        let fallback_config = FileSystemConfig::new(
            ConnectionUri::parse("file:///fallback").expect("fallback URI must parse"),
        );
        self.inner.create_configured(&fallback_config)
    }
}
