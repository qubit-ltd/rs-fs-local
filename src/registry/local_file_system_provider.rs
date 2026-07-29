//! Registry provider for host and rooted local filesystems.

use std::path::{
    Path,
    PathBuf,
};

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
    ProviderMetadata,
    ServiceProvider,
    provider_descriptor,
};

use crate::LocalFileSystems;

/// Creates local filesystem resolutions for accepted `file:` configurations.
#[derive(Clone, Debug)]
pub struct LocalFileSystemProvider {
    mode: LocalProviderMode,
}

/// Selects host-wide or descriptor-rooted authority for a provider instance.
#[derive(Clone, Debug)]
enum LocalProviderMode {
    Host,
    Rooted { id: FileSystemId, root: PathBuf },
}

impl LocalFileSystemProvider {
    /// Creates a host-wide local provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: LocalProviderMode::Host,
        }
    }

    /// Creates a provider restricted to `root` with the supplied stable
    /// identity.
    #[must_use]
    pub fn rooted(id: FileSystemId, root: &Path) -> Self {
        Self {
            mode: LocalProviderMode::Rooted {
                id,
                root: root.to_path_buf(),
            },
        }
    }

    /// Validates and decodes a registry configuration into a logical path and
    /// URI.
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
            .expose_unredacted(Uri::parse)
            .map_err(ProviderFailure::invalid_configuration)?;
        if uri.scheme() != "file" {
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
        let path = FsPath::parse(uri.path())
            .map_err(ProviderFailure::invalid_configuration)?;
        if !path.is_absolute() {
            return Err(invalid_path("local file URI path must be absolute"));
        }
        Ok((path, uri))
    }
}

impl Default for LocalFileSystemProvider {
    /// Creates the default host-wide provider.
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderMetadata for LocalFileSystemProvider {
    /// Returns the stable local-provider descriptor and the `file` alias.
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("local-file", aliases: ["file"])
    }
}

impl ServiceProvider<FileSystemSpec> for LocalFileSystemProvider {
    /// Resolves one accepted `file:` configuration to a concrete facade.
    fn create_configured(
        &self,
        config: &FileSystemConfig,
    ) -> Result<FileSystemResolution, ProviderFailure<FsError>> {
        let (path, uri) = Self::decode_config(config)?;
        let file_system = match &self.mode {
            LocalProviderMode::Host => LocalFileSystems::host(),
            LocalProviderMode::Rooted { id, root } => {
                LocalFileSystems::rooted_with_id(id.clone(), root)
            }
        }
        .map_err(ProviderFailure::initialization_failed)?;
        FileSystemResolution::try_new(file_system, path, uri)
            .map_err(ProviderFailure::initialization_failed)
    }
}

/// Builds an invalid-options provider failure.
fn invalid_options(message: &'static str) -> ProviderFailure<FsError> {
    ProviderFailure::invalid_configuration(FsError::new(
        FsErrorKind::InvalidOptions,
        FsOperation::Provider,
        message,
    ))
}

/// Builds an invalid-path provider failure.
fn invalid_path(message: &'static str) -> ProviderFailure<FsError> {
    ProviderFailure::invalid_configuration(FsError::new(
        FsErrorKind::InvalidPath,
        FsOperation::Provider,
        message,
    ))
}
