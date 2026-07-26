// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Defines the synchronous local filesystem implementation.

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
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
    AchievedAtomicity,
    AtomicityRequirement,
    CopyConflictPolicy,
    CopyMethod,
    CopyMode,
    CopyOptions,
    CopyOutcome,
    CopyStats,
    CreateDirOptions,
    DeleteOptions,
    DirectoryStream,
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
    ListOptions,
    MetadataPreservePolicy,
    NativePathCodec,
    NativePathCodecError,
    OpenedFileInfo,
    OsStrPathCodec,
    PathSemantics,
    PublicationMethod,
    ReadOptions,
    RenameOptions,
    RenameOutcome,
    WriteDisposition,
    WriteOptions,
};
use qubit_local_files::{
    atomic,
    copy,
    directory,
    read,
    remove,
    rename,
    write,
};

use crate::internal::{
    LocalDirectoryStreamSession,
    LocalFileWriteSession,
    validate_hierarchical_path,
};

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
        let mut capabilities = FileSystemCapabilities::default()
            .with(FileSystemCapability::List)
            .with(FileSystemCapability::Read)
            .with(FileSystemCapability::Write)
            .with(FileSystemCapability::Append)
            .with(FileSystemCapability::CreateDirectory)
            .with(FileSystemCapability::Delete)
            .with(FileSystemCapability::RecursiveDelete)
            .with(FileSystemCapability::Rename)
            .with(FileSystemCapability::AtomicReplace)
            .with(FileSystemCapability::Copy);
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        capabilities.insert(FileSystemCapability::AtomicRename);
        Self {
            info,
            capabilities,
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
    /// Returns an invalid-path error when `path` is relative or not already
    /// normalized, or when a decoded component changes its native boundary.
    ///
    /// # Panics
    ///
    /// Panics only if validated [`FsPath`] text violates the native path codec
    /// invariant.
    fn native_path(operation: FsOperation, path: &FsPath) -> FsResult<PathBuf> {
        validate_hierarchical_path(operation, path)?;
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
    pub(crate) fn map_metadata(metadata: fs::Metadata) -> FileMetadata {
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
    pub(crate) fn map_io_error(
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
    /// Opens a deterministic snapshot of a host-local directory listing.
    ///
    /// # Parameters
    ///
    /// * `path` - Absolute directory path to enumerate.
    /// * `options` - Recursion, symbolic-link, metadata, and prefix policies.
    ///
    /// # Returns
    ///
    /// A type-erased stream over matching canonical directory entries.
    ///
    /// # Errors
    ///
    /// Returns a path-aware filesystem error when the path is invalid, the
    /// directory cannot be read, or entry metadata cannot be represented.
    fn list(
        &self,
        path: &FsPath,
        options: ListOptions,
    ) -> FsResult<DirectoryStream> {
        let native_path = Self::native_path(FsOperation::List, path)?;
        let session =
            LocalDirectoryStreamSession::capture(native_path, path, options)?;
        Ok(DirectoryStream::new(session))
    }

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
        let reader = read::open(&native_path).map_err(|error| {
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
                atomic::Options::new().with_parent()
            } else {
                atomic::Options::new()
            };
            let writer = atomic::begin_with(&native_path, atomic_options)
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
                WriteDisposition::CreateNew => write::Mode::CreateNew,
                WriteDisposition::CreateOrReplace => {
                    write::Mode::CreateOrTruncate
                }
                WriteDisposition::Append => write::Mode::AppendExisting,
            };
            let local_options = if options.create_parent {
                write::OpenOptions::new(mode).with_parents()
            } else {
                write::OpenOptions::new(mode)
            };
            let writer =
                write::open(&native_path, &local_options).map_err(|error| {
                    Self::map_io_error(FsOperation::OpenWriter, path, error)
                })?;
            LocalFileWriteSession::direct(writer, path.clone())
        };
        let location = FileLocation::new(self.info.id().clone(), path.clone());
        Ok(FileWriter::new(session, OpenedFileInfo::new(location)))
    }

    /// Creates one host-local directory.
    ///
    /// # Parameters
    ///
    /// * `path` - Absolute directory path to create.
    /// * `options` - Parent creation and existing-directory policies.
    ///
    /// # Errors
    ///
    /// Returns an invalid-options error for user metadata, or a path-aware I/O
    /// error when the directory cannot be created.
    fn create_dir(
        &self,
        path: &FsPath,
        options: CreateDirOptions,
    ) -> FsResult<()> {
        if options.user_metadata.as_metadata().iter().next().is_some() {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::CreateDir,
                "local directories do not support user metadata",
            )
            .with_path(path.clone())
            .with_provider(Self::provider_id()));
        }
        let native_path = Self::native_path(FsOperation::CreateDir, path)?;
        if options.recursive {
            directory::create_parent(&native_path).map_err(|error| {
                Self::map_io_error(FsOperation::CreateDir, path, error)
            })?;
        }
        match directory::create(&native_path) {
            Ok(()) => Ok(()),
            Err(error)
                if options.exists_ok
                    && error.kind() == io::ErrorKind::AlreadyExists =>
            {
                let metadata =
                    fs::symlink_metadata(&native_path).map_err(|error| {
                        Self::map_io_error(FsOperation::CreateDir, path, error)
                    })?;
                if metadata.is_dir() {
                    Ok(())
                } else {
                    Err(Self::map_io_error(FsOperation::CreateDir, path, error))
                }
            }
            Err(error) => {
                Err(Self::map_io_error(FsOperation::CreateDir, path, error))
            }
        }
    }

    /// Deletes one host-local entry with explicit recursive and absence policy.
    ///
    /// # Parameters
    ///
    /// * `path` - Absolute path to delete.
    /// * `options` - Recursive, missing-target, and conditional policies.
    ///
    /// # Errors
    ///
    /// Returns a requirement error before I/O for unsupported conditional
    /// deletion, or a path-aware I/O error when removal fails.
    fn delete(&self, path: &FsPath, options: DeleteOptions) -> FsResult<()> {
        options
            .validate_against(self.capabilities)
            .map_err(|error| {
                error
                    .with_path(path.clone())
                    .with_provider(Self::provider_id())
            })?;
        let native_path = Self::native_path(FsOperation::Delete, path)?;
        let metadata = match fs::symlink_metadata(&native_path) {
            Ok(metadata) => metadata,
            Err(error)
                if options.missing_ok
                    && error.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(());
            }
            Err(error) => {
                return Err(Self::map_io_error(
                    FsOperation::Delete,
                    path,
                    error,
                ));
            }
        };
        let result = if metadata.is_dir() {
            if options.recursive {
                remove::directory_tree(&native_path)
            } else {
                remove::empty_directory(&native_path)
            }
        } else {
            remove::file(&native_path)
        };
        match result {
            Ok(()) => Ok(()),
            Err(error)
                if options.missing_ok
                    && error.kind() == io::ErrorKind::NotFound =>
            {
                Ok(())
            }
            Err(error) => {
                Err(Self::map_io_error(FsOperation::Delete, path, error))
            }
        }
    }

    /// Renames one host-local entry within the host filesystem namespace.
    ///
    /// # Parameters
    ///
    /// * `from` - Absolute source path.
    /// * `to` - Absolute destination path.
    /// * `options` - Destination overwrite and required atomicity policies.
    ///
    /// # Returns
    ///
    /// An atomic-rename outcome after successful native publication.
    ///
    /// # Errors
    ///
    /// Returns a requirement error before I/O when required atomic rename is
    /// unavailable, or a source/target-aware error when native rename fails.
    fn rename(
        &self,
        from: &FsPath,
        to: &FsPath,
        options: RenameOptions,
    ) -> FsResult<RenameOutcome> {
        options
            .validate_against(self.capabilities)
            .map_err(|error| {
                error
                    .with_path(from.clone())
                    .with_target(to.clone())
                    .with_provider(Self::provider_id())
            })?;
        let native_from = Self::native_path(FsOperation::Rename, from)?;
        let native_to = Self::native_path(FsOperation::Rename, to)?;
        let result = if options.overwrite {
            rename::move_path(&native_from, &native_to)
        } else {
            rename::move_path_without_replacing(&native_from, &native_to)
        };
        result.map_err(|error| {
            Self::map_io_error(FsOperation::Rename, from, error)
                .with_target(to.clone())
        })?;
        Ok(RenameOutcome::new(
            AchievedAtomicity::Atomic,
            PublicationMethod::AtomicRename,
        ))
    }

    /// Copies one file or directory tree within the host filesystem namespace.
    ///
    /// # Parameters
    ///
    /// * `from` - Absolute source path.
    /// * `to` - Absolute destination path.
    /// * `options` - Source mode, conflict, metadata, link, and parent policy.
    ///
    /// # Returns
    ///
    /// Local copy statistics and the non-atomic publication guarantee.
    ///
    /// # Errors
    ///
    /// Returns an option or capability error before destination changes when
    /// requested semantics are unsupported, or a source/target-aware I/O error
    /// when native copying fails.
    fn copy(
        &self,
        from: &FsPath,
        to: &FsPath,
        options: CopyOptions,
    ) -> FsResult<CopyOutcome> {
        options
            .validate_against(self.capabilities)
            .map_err(|error| {
                error
                    .with_path(from.clone())
                    .with_target(to.clone())
                    .with_provider(Self::provider_id())
            })?;
        if options.continue_on_error {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::Copy,
                "local copy does not support continue-on-error",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(Self::provider_id()));
        }
        if !matches!(
            options.preserve_metadata,
            MetadataPreservePolicy::None | MetadataPreservePolicy::Portable
        ) {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::Copy,
                "local copy supports only none or portable metadata preservation",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(Self::provider_id()));
        }
        if from == to {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::Copy,
                "local copy source and destination must differ",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(Self::provider_id()));
        }
        let native_from = Self::native_path(FsOperation::Copy, from)?;
        let native_to = Self::native_path(FsOperation::Copy, to)?;
        let link_metadata =
            fs::symlink_metadata(&native_from).map_err(|error| {
                Self::map_io_error(FsOperation::Copy, from, error)
                    .with_target(to.clone())
            })?;
        if link_metadata.file_type().is_symlink() && !options.follow_symlinks {
            return Err(FsError::new(
                FsErrorKind::UnsupportedCapability,
                FsOperation::Copy,
                "local copy does not copy symbolic-link entries",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(Self::provider_id())
            .with_required_capability(FileSystemCapability::Symlink));
        }
        let metadata = if link_metadata.file_type().is_symlink() {
            fs::metadata(&native_from).map_err(|error| {
                Self::map_io_error(FsOperation::Copy, from, error)
                    .with_target(to.clone())
            })?
        } else {
            link_metadata
        };
        let copy_tree = match options.mode {
            CopyMode::Auto => metadata.is_dir(),
            CopyMode::File if metadata.is_file() => false,
            CopyMode::Tree if metadata.is_dir() => true,
            CopyMode::File | CopyMode::Tree => {
                return Err(FsError::new(
                    FsErrorKind::InvalidOptions,
                    FsOperation::Copy,
                    "copy mode does not match the source entry type",
                )
                .with_path(from.clone())
                .with_target(to.clone())
                .with_provider(Self::provider_id()));
            }
        };
        if options.create_parent {
            directory::create_parent(&native_to).map_err(|error| {
                Self::map_io_error(FsOperation::Copy, to, error)
                    .with_target(to.clone())
            })?;
        }
        if copy_tree {
            let conflict = match options.conflict {
                CopyConflictPolicy::Fail => copy::ConflictPolicy::Fail,
                CopyConflictPolicy::Overwrite => {
                    copy::ConflictPolicy::Overwrite
                }
                CopyConflictPolicy::Skip => copy::ConflictPolicy::Skip,
            };
            let type_conflict =
                if options.conflict == CopyConflictPolicy::Overwrite {
                    copy::TypeConflictPolicy::Replace
                } else {
                    copy::TypeConflictPolicy::Fail
                };
            let mut local_options = copy::Options::new()
                .with_conflict(conflict)
                .with_type_conflict(type_conflict);
            if options.follow_symlinks {
                local_options = local_options.follow_symlinks();
            }
            if options.preserve_metadata == MetadataPreservePolicy::Portable {
                local_options = local_options.preserve_permissions();
            }
            let statistics =
                copy::directory(&native_from, &native_to, local_options)
                    .map_err(|error| {
                        let kind = error.kind();
                        FsError::from_io(
                            io::Error::new(kind, error),
                            FsOperation::Copy,
                        )
                        .with_path(from.clone())
                        .with_target(to.clone())
                        .with_provider(Self::provider_id())
                    })?;
            let stats = CopyStats {
                files: statistics.files(),
                directories: statistics.directories(),
                bytes: statistics.bytes(),
                skipped: statistics.skipped(),
                ..CopyStats::default()
            };
            return Ok(CopyOutcome::new(
                stats,
                CopyMethod::Local,
                AchievedAtomicity::NonAtomic,
            ));
        }
        let destination_metadata = match fs::symlink_metadata(&native_to) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(Self::map_io_error(FsOperation::Copy, to, error)
                    .with_target(to.clone()));
            }
        };
        #[cfg(unix)]
        if destination_metadata.as_ref().is_some_and(|destination| {
            metadata.dev() == destination.dev()
                && metadata.ino() == destination.ino()
        }) {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::Copy,
                "local copy source and destination identify the same file",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(Self::provider_id()));
        }
        let destination_existed = destination_metadata.is_some();
        if options.conflict == CopyConflictPolicy::Skip && destination_existed {
            return Ok(CopyOutcome::new(
                CopyStats {
                    skipped: 1,
                    ..CopyStats::default()
                },
                CopyMethod::Local,
                AchievedAtomicity::NonAtomic,
            ));
        }
        if options.conflict == CopyConflictPolicy::Overwrite
            && destination_metadata
                .as_ref()
                .is_some_and(|metadata| metadata.file_type().is_symlink())
        {
            remove::file(&native_to).map_err(|error| {
                Self::map_io_error(FsOperation::Copy, to, error)
                    .with_target(to.clone())
            })?;
        }
        let copied = match options.conflict {
            CopyConflictPolicy::Overwrite => {
                copy::file(&native_from, &native_to)
            }
            CopyConflictPolicy::Fail | CopyConflictPolicy::Skip => {
                copy::file_without_replacing(&native_from, &native_to)
            }
        }
        .map_err(|error| {
            Self::map_io_error(FsOperation::Copy, from, error)
                .with_target(to.clone())
        })?;
        Ok(CopyOutcome::new(
            CopyStats {
                files: 1,
                bytes: copied,
                overwritten: u64::from(
                    destination_existed
                        && options.conflict == CopyConflictPolicy::Overwrite,
                ),
                ..CopyStats::default()
            },
            CopyMethod::Local,
            AchievedAtomicity::NonAtomic,
        ))
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
