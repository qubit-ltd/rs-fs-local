// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Defines the synchronous local filesystem implementation.

use std::{
    fs,
    io,
    path::{
        Component,
        Path,
        PathBuf,
    },
};

use qubit_fs::{
    AtomicityRequirement,
    FileKind,
    FileLocation,
    FileMetadata,
    FileReader,
    FileSystem,
    FileSystemCapabilities,
    FileSystemCapability,
    FileSystemId,
    FileSystemInfo,
    FileSystemLimits,
    FileSystemProperties,
    FileWriter,
    FsError,
    FsErrorKind,
    FsOperation,
    FsPath,
    FsResult,
    NativePathCodec,
    NativePathCodecError,
    OpenedFileInfo,
    OsStrPathCodec,
    PathSemantics,
    ReadOptions,
    WriteDisposition,
    WriteOptions,
};
use qubit_local_files::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalAtomicWriteOptions,
    LocalFiles,
};

use crate::internal::LocalFileWriteSession;

/// Provides the synchronous `file:` filesystem implementation.
///
/// This type has host-wide authority and accepts only native absolute paths.
/// It does not provide rooted sandbox semantics.
pub struct LocalFileSystem {
    /// Immutable provider information returned without I/O.
    info: FileSystemInfo,
    /// Immutable operation guarantees returned without I/O.
    capabilities: FileSystemCapabilities,
    /// Stable provider limits returned without I/O.
    limits: FileSystemLimits,
}

impl LocalFileSystem {
    /// Creates a filesystem with authority over host-native absolute paths.
    ///
    /// # Returns
    ///
    /// A filesystem whose paths use hierarchical local semantics.
    ///
    /// # Panics
    ///
    /// Panics only if a static filesystem id, provider id, or URI scheme
    /// violates its corresponding grammar.
    #[must_use]
    #[inline]
    pub fn host() -> Self {
        let id = FileSystemId::new("local-host")
            .expect("static local filesystem id must be valid");
        let provider_id = Self::provider_id();
        let info =
            FileSystemInfo::new(id, provider_id, PathSemantics::Hierarchical)
                .with_scheme("file")
                .expect("static file URI scheme must be valid");
        Self {
            info,
            capabilities: FileSystemCapabilities::default()
                .with(FileSystemCapability::Read)
                .with(FileSystemCapability::Write)
                .with(FileSystemCapability::Append)
                .with(FileSystemCapability::AtomicReplace),
            limits: FileSystemLimits::unknown(),
        }
    }

    /// Converts one absolute native path into canonical filesystem path text.
    ///
    /// Native roots and separators are interpreted by [`Path::components`],
    /// while each ordinary component is decoded independently. On Windows only
    /// drive-letter paths are accepted; UNC and device prefixes remain outside
    /// this provider's host-path contract.
    ///
    /// # Parameters
    ///
    /// * `path` - Absolute path in the current operating system's native form.
    ///
    /// # Returns
    ///
    /// An absolute canonical [`FsPath`] using `/` component separators.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error when `path` is relative, contains a parent
    /// component, uses an unsupported native prefix, or a component cannot be
    /// decoded losslessly.
    pub fn path_from_native(path: &Path) -> FsResult<FsPath> {
        if !path.is_absolute() {
            return Err(Self::invalid_native_path(
                FsOperation::ParsePath,
                "local filesystem path must be absolute",
            ));
        }
        let canonical = Self::decode_native_components(path)?;
        FsPath::parse(&canonical).map_err(|error| {
            FsError::with_source(
                FsErrorKind::InvalidPath,
                FsOperation::ParsePath,
                "native path is not a valid canonical filesystem path",
                error,
            )
            .with_provider(Self::provider_id())
        })
    }

    /// Returns the identifier shared by the filesystem and its provider.
    ///
    /// # Returns
    ///
    /// The validated static `local-file` provider identifier.
    ///
    /// # Panics
    ///
    /// Panics only if the static provider identifier violates the provider-id
    /// grammar.
    #[inline]
    pub(crate) const fn provider_id() -> &'static str {
        "local-file"
    }

    /// Converts a canonical filesystem path into its native representation.
    ///
    /// # Parameters
    ///
    /// * `operation` - Public filesystem operation that needs the path.
    /// * `path` - Canonical filesystem path to convert.
    ///
    /// # Returns
    ///
    /// The losslessly reconstructed native path.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error when the decoded native path is relative.
    ///
    /// # Panics
    ///
    /// Panics only if validated [`FsPath`] text violates the native path codec
    /// invariant.
    fn native_path(operation: FsOperation, path: &FsPath) -> FsResult<PathBuf> {
        if !path.is_absolute() {
            return Err(Self::invalid_native_path(
                operation,
                "local filesystem path must be absolute",
            )
            .with_path(path.clone()));
        }
        Self::encode_canonical_components(operation, path)
    }

    /// Decodes native components without allowing lexical parent traversal.
    ///
    /// # Parameters
    ///
    /// * `path` - Validated absolute native path.
    ///
    /// # Returns
    ///
    /// Canonical `/`-separated path text.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error for parent components, unsupported native
    /// prefixes, or lossless codec failures.
    fn decode_native_components(path: &Path) -> FsResult<String> {
        let mut canonical = String::new();
        for component in path.components() {
            match component {
                Component::Prefix(prefix) => {
                    Self::decode_native_prefix(&mut canonical, prefix)?;
                }
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir => {
                    return Err(Self::invalid_native_path(
                        FsOperation::ParsePath,
                        "native path must not contain parent traversal",
                    ));
                }
                Component::Normal(component) => {
                    let component = OsStrPathCodec.decode(component).map_err(|error| {
                        Self::native_codec_error(
                            FsOperation::ParsePath,
                            "native path component cannot be decoded losslessly",
                            error,
                        )
                    })?;
                    if canonical.is_empty() || !canonical.ends_with('/') {
                        canonical.push('/');
                    }
                    canonical.push_str(component.as_ref());
                }
            }
        }
        if canonical.is_empty() {
            canonical.push('/');
        }
        Ok(canonical)
    }

    /// Adds an operating-system prefix to canonical path text.
    ///
    /// # Parameters
    ///
    /// * `canonical` - Canonical text under construction.
    /// * `prefix` - Native path prefix reported by [`Path::components`].
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error when the current platform reports a prefix
    /// that this provider does not support.
    #[cfg(windows)]
    fn decode_native_prefix(
        canonical: &mut String,
        prefix: std::path::PrefixComponent<'_>,
    ) -> FsResult<()> {
        use std::path::Prefix;

        let drive = match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            _ => {
                return Err(Self::invalid_native_path(
                    FsOperation::ParsePath,
                    "local filesystem path uses an unsupported Windows prefix",
                ));
            }
        };
        canonical.push('/');
        canonical.push(char::from(drive).to_ascii_uppercase());
        canonical.push(':');
        Ok(())
    }

    /// Rejects native prefixes on platforms where they are not meaningful.
    ///
    /// # Parameters
    ///
    /// * `canonical` - Canonical text under construction.
    /// * `prefix` - Unexpected native prefix.
    ///
    /// # Errors
    ///
    /// Always returns an invalid-path error.
    #[cfg(not(windows))]
    fn decode_native_prefix(
        _canonical: &mut String,
        _prefix: std::path::PrefixComponent<'_>,
    ) -> FsResult<()> {
        Err(Self::invalid_native_path(
            FsOperation::ParsePath,
            "local filesystem path uses an unsupported native prefix",
        ))
    }

    /// Encodes canonical components into a native absolute path.
    ///
    /// # Parameters
    ///
    /// * `operation` - Public filesystem operation using the path.
    /// * `path` - Absolute canonical filesystem path.
    ///
    /// # Returns
    ///
    /// Native path safe to pass to local filesystem APIs.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error when the canonical root is unsupported or
    /// a component cannot be encoded without changing its boundary.
    #[cfg(not(windows))]
    fn encode_canonical_components(
        operation: FsOperation,
        path: &FsPath,
    ) -> FsResult<PathBuf> {
        let mut native = PathBuf::from("/");
        for component in
            path.as_str().split('/').filter(|value| !value.is_empty())
        {
            let component =
                OsStrPathCodec.encode(component).map_err(|error| {
                    Self::native_codec_error(
                        operation,
                        "canonical path component cannot be encoded losslessly",
                        error,
                    )
                    .with_path(path.clone())
                })?;
            let component: &std::ffi::OsStr = component.as_ref();
            native.push(component);
        }
        Ok(native)
    }

    /// Encodes a canonical Windows drive path component by component.
    ///
    /// # Parameters
    ///
    /// * `operation` - Public filesystem operation using the path.
    /// * `path` - Absolute canonical filesystem path.
    ///
    /// # Returns
    ///
    /// Native drive path safe to pass to Windows filesystem APIs.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error for a missing drive prefix or for a
    /// component containing a native separator.
    #[cfg(windows)]
    fn encode_canonical_components(
        operation: FsOperation,
        path: &FsPath,
    ) -> FsResult<PathBuf> {
        let mut components =
            path.as_str().split('/').filter(|value| !value.is_empty());
        let drive = components.next().unwrap_or_default();
        let drive_bytes = drive.as_bytes();
        if drive_bytes.len() != 2
            || !drive_bytes[0].is_ascii_alphabetic()
            || drive_bytes[1] != b':'
        {
            return Err(Self::invalid_native_path(
                operation,
                "local Windows path must begin with an absolute drive prefix",
            )
            .with_path(path.clone()));
        }
        let mut native = PathBuf::from(format!(
            "{}:\\",
            char::from(drive_bytes[0]).to_ascii_uppercase(),
        ));
        for component in components {
            if component.contains('\\') {
                return Err(Self::invalid_native_path(
                    operation,
                    "canonical path component contains a Windows separator",
                )
                .with_path(path.clone()));
            }
            let component =
                OsStrPathCodec.encode(component).map_err(|error| {
                    Self::native_codec_error(
                        operation,
                        "canonical path component cannot be encoded losslessly",
                        error,
                    )
                    .with_path(path.clone())
                })?;
            let component: &std::ffi::OsStr = component.as_ref();
            native.push(component);
        }
        Ok(native)
    }

    /// Creates a provider-aware invalid native path error.
    ///
    /// # Parameters
    ///
    /// * `operation` - Operation that rejected the path.
    /// * `message` - Stable non-sensitive rejection reason.
    ///
    /// # Returns
    ///
    /// Invalid-path error carrying the local provider id.
    #[inline]
    fn invalid_native_path(operation: FsOperation, message: &str) -> FsError {
        FsError::new(FsErrorKind::InvalidPath, operation, message)
            .with_provider(Self::provider_id())
    }

    /// Creates a provider-aware native codec error.
    ///
    /// # Parameters
    ///
    /// * `operation` - Operation that failed during conversion.
    /// * `message` - Stable non-sensitive failure description.
    /// * `error` - Lossless native codec failure retained as source.
    ///
    /// # Returns
    ///
    /// Invalid-path error carrying provider and source context.
    #[inline]
    fn native_codec_error(
        operation: FsOperation,
        message: &str,
        error: NativePathCodecError,
    ) -> FsError {
        FsError::with_source(
            FsErrorKind::InvalidPath,
            operation,
            message,
            error,
        )
        .with_provider(Self::provider_id())
    }

    /// Converts one native metadata result into provider-neutral metadata.
    ///
    /// # Parameters
    ///
    /// * `metadata` - Native metadata captured without following a final link.
    ///
    /// # Returns
    ///
    /// The corresponding file kind, byte length, and available timestamps.
    fn map_metadata(metadata: fs::Metadata) -> FileMetadata {
        let file_type = metadata.file_type();
        let kind = if file_type.is_file() {
            FileKind::File
        } else if file_type.is_dir() {
            FileKind::Directory
        } else if file_type.is_symlink() {
            FileKind::Symlink
        } else {
            FileKind::Other("native-special".to_owned())
        };
        let mut result = FileMetadata::new(kind);
        result.len = Some(metadata.len());
        result.modified_at = metadata.modified().ok();
        result.created_at = metadata.created().ok();
        result.accessed_at = metadata.accessed().ok();
        result
    }

    /// Converts one local I/O failure into a path-aware filesystem error.
    ///
    /// # Parameters
    ///
    /// * `operation` - Public filesystem operation that failed.
    /// * `path` - Provider-local path involved in the operation.
    /// * `error` - Native I/O error to retain as an opaque source.
    ///
    /// # Returns
    ///
    /// A provider-neutral error with a scrubbed message and native source.
    #[inline(always)]
    fn map_io_error(
        operation: FsOperation,
        path: &FsPath,
        error: io::Error,
    ) -> FsError {
        FsError::from_io(error, operation)
            .with_path(path.clone())
            .with_provider(Self::provider_id())
    }
}

impl FileSystemProperties for LocalFileSystem {
    /// Returns immutable local provider information without performing I/O.
    ///
    /// # Returns
    ///
    /// Identity and path semantics fixed when this filesystem was constructed.
    #[inline(always)]
    fn info(&self) -> &FileSystemInfo {
        &self.info
    }

    /// Returns immutable local capability guarantees without performing I/O.
    ///
    /// # Returns
    ///
    /// The read, write, append, and atomic-replace capability snapshot fixed
    /// at construction time.
    #[inline(always)]
    fn capabilities(&self) -> FileSystemCapabilities {
        self.capabilities
    }

    /// Returns stable local provider limits without performing I/O.
    ///
    /// # Returns
    ///
    /// The explicit host-dependent limit snapshot fixed at construction time.
    #[inline(always)]
    fn limits(&self) -> &FileSystemLimits {
        &self.limits
    }
}

impl FileSystem for LocalFileSystem {
    /// Opens a native regular file for synchronous sequential reading.
    ///
    /// This method performs blocking local filesystem I/O. It validates all
    /// requested read semantics before attempting to inspect or open the path.
    ///
    /// # Parameters
    ///
    /// * `path` - Absolute provider-local path to open.
    /// * `options` - Required read semantics; only whole-file reads are
    ///   supported.
    ///
    /// # Returns
    ///
    /// An unbuffered reader bound to the requested local filesystem location.
    ///
    /// # Errors
    ///
    /// Returns a requirement error when range, conditional, or required
    /// checksum semantics are requested. Returns a path-aware filesystem error
    /// when the path is relative, missing, inaccessible, or not a regular file.
    fn open_reader(
        &self,
        path: &FsPath,
        options: ReadOptions,
    ) -> FsResult<FileReader> {
        options
            .validate_against(self.capabilities)
            .map_err(|error| {
                error
                    .with_path(path.clone())
                    .with_provider(Self::provider_id())
            })?;
        let native_path = Self::native_path(FsOperation::OpenReader, path)?;
        let reader =
            LocalFiles::open_reader(native_path, FileReadOptions::unbuffered())
                .map_err(|error| {
                    Self::map_io_error(FsOperation::OpenReader, path, error)
                })?;
        let location = FileLocation::new(self.info.id().clone(), path.clone());
        let info = OpenedFileInfo::new(location);
        Ok(FileReader::new(reader, info))
    }

    /// Opens a synchronous local file write session.
    ///
    /// Preferred and required create-or-replace writes use same-directory
    /// atomic publication. Explicitly non-atomic replacement, append, and
    /// create-new requests use direct native writers.
    ///
    /// # Parameters
    ///
    /// * `path` - Absolute provider-local destination path.
    /// * `options` - Required disposition, atomicity, and parent policy.
    ///
    /// # Returns
    /// An open writer bound to the requested local filesystem location.
    ///
    /// # Errors
    ///
    /// Returns an option or requirement error before side effects when the
    /// requested contract is unsupported. Returns a path-aware filesystem
    /// error when the native path cannot be prepared or opened.
    fn open_writer(
        &self,
        path: &FsPath,
        options: WriteOptions,
    ) -> FsResult<FileWriter> {
        options
            .validate_against(self.capabilities)
            .map_err(|error| {
                error
                    .with_path(path.clone())
                    .with_provider(Self::provider_id())
            })?;
        if options.content_type.is_some()
            || options.user_metadata.as_metadata().iter().next().is_some()
            || options.checksum.is_some()
        {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::OpenWriter,
                "local writes do not support content metadata or checksums",
            )
            .with_path(path.clone())
            .with_provider(Self::provider_id()));
        }
        if options.disposition == WriteDisposition::CreateNew
            && options.atomicity == AtomicityRequirement::Required
        {
            return Err(FsError::new(
                FsErrorKind::RequirementNotMet,
                FsOperation::OpenWriter,
                "atomic create-new publication is not supported",
            )
            .with_path(path.clone())
            .with_provider(Self::provider_id())
            .with_required_capability(FileSystemCapability::AtomicReplace));
        }

        let native_path = Self::native_path(FsOperation::OpenWriter, path)?;
        let session = if options.disposition
            == WriteDisposition::CreateOrReplace
            && options.atomicity != AtomicityRequirement::NotRequired
        {
            let atomic_options = if options.create_parent {
                LocalAtomicWriteOptions::new().with_parent()
            } else {
                LocalAtomicWriteOptions::new()
            };
            let writer = LocalFiles::begin_atomic_write_with_options(
                native_path,
                atomic_options,
            )
            .map_err(|error| {
                let kind = error.kind();
                FsError::from_io(
                    io::Error::new(kind, error),
                    FsOperation::OpenWriter,
                )
                .with_path(path.clone())
                .with_provider(Self::provider_id())
            })?;
            LocalFileWriteSession::atomic(writer, path.clone())
        } else {
            let mode = match options.disposition {
                WriteDisposition::CreateNew => FileWriteMode::CreateNew,
                WriteDisposition::CreateOrReplace => {
                    FileWriteMode::CreateOrTruncate
                }
                WriteDisposition::Append => FileWriteMode::AppendExisting,
            };
            let local_options = if options.create_parent {
                FileWriteOptions::new(mode).with_parent()
            } else {
                FileWriteOptions::new(mode)
            };
            let writer = LocalFiles::open_writer(native_path, local_options)
                .map_err(|error| {
                    Self::map_io_error(FsOperation::OpenWriter, path, error)
                })?;
            LocalFileWriteSession::direct(writer, path.clone())
        };
        let location = FileLocation::new(self.info.id().clone(), path.clone());
        Ok(FileWriter::new(session, OpenedFileInfo::new(location)))
    }

    /// Reads native metadata for one host-wide local path.
    ///
    /// This method performs blocking local filesystem I/O.
    ///
    /// # Parameters
    ///
    /// * `path` - Absolute provider-local path to inspect.
    ///
    /// # Returns
    ///
    /// Metadata captured without following the final symbolic link.
    ///
    /// # Errors
    ///
    /// Returns a path-aware filesystem error when native metadata lookup fails.
    fn stat(&self, path: &FsPath) -> FsResult<FileMetadata> {
        let native_path = Self::native_path(FsOperation::Stat, path)?;
        fs::symlink_metadata(native_path)
            .map(Self::map_metadata)
            .map_err(|error| Self::map_io_error(FsOperation::Stat, path, error))
    }
}
