// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative local filesystem adapter.

use std::{
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
use qubit_local_files::backend::{
    atomic,
    copy,
    read,
    rooted,
    write,
};

use crate::{
    LocalFileSystem,
    internal::{
        RootedDirectoryStreamSession,
        RootedFileWriteSession,
        validate_hierarchical_path,
    },
};

/// A local filesystem whose authority is anchored to an opened directory.
///
/// Provider paths use canonical absolute syntax; the leading slash denotes
/// the opened root rather than the host operating-system root.
pub struct RootedLocalFileSystem {
    root: rooted::Root,
    info: FileSystemInfo,
    capabilities: FileSystemCapabilities,
    limits: FileSystemLimits,
}

impl RootedLocalFileSystem {
    /// Opens a descriptor-relative filesystem rooted at `path` with `id`.
    ///
    /// The caller must supply the stable identity of this configured root so
    /// opened locations from different roots cannot collide.
    ///
    /// # Parameters
    ///
    /// * `id` - Stable identity of this configured rooted filesystem.
    /// * `path` - Directory whose opened descriptor becomes the authority.
    ///
    /// # Returns
    /// A filesystem that accepts canonical absolute paths below `path`.
    ///
    /// # Errors
    /// Returns an I/O error when the root cannot be securely opened or when
    /// descriptor-relative roots are unsupported on the current platform.
    pub fn open(id: FileSystemId, path: &Path) -> std::io::Result<Self> {
        let root = rooted::Root::open(path)?;
        let info = FileSystemInfo::new(
            id,
            LocalFileSystem::provider_id(),
            PathSemantics::Hierarchical,
        );
        let capabilities = FileSystemCapabilities::default()
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
        #[cfg(any(
            windows,
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
        ))]
        let capabilities = {
            let mut capabilities = capabilities;
            capabilities.insert(FileSystemCapability::AtomicRename);
            capabilities
        };
        Ok(Self {
            root,
            info,
            capabilities,
            limits: FileSystemLimits::unknown(),
        })
    }

    /// Converts canonical rooted syntax to a validated native relative path.
    ///
    /// Each canonical component is decoded independently through
    /// [`OsStrPathCodec`]. Decoded native separators, roots, prefixes, and dot
    /// components are rejected before the value reaches the rooted API.
    ///
    /// # Parameters
    ///
    /// * `path` - Absolute canonical path below this filesystem root.
    /// * `operation` - Public operation used to build an error context.
    ///
    /// # Returns
    /// A non-empty rooted native path.
    ///
    /// # Errors
    /// Returns an invalid-path error for a relative/root path or a component
    /// that cannot be decoded without changing its native boundary.
    pub(crate) fn relative_path(
        path: &FsPath,
        operation: FsOperation,
    ) -> FsResult<rooted::Path> {
        validate_hierarchical_path(operation, path)?;
        let relative = path
            .as_str()
            .strip_prefix('/')
            .expect("validated absolute path must have a leading slash");
        if relative.is_empty() {
            return Err(FsError::invalid_path(
                operation,
                "rooted local filesystem root cannot be opened as a file",
            )
            .with_path(path.clone())
            .with_provider(LocalFileSystem::provider_id()));
        }
        let mut native = PathBuf::new();
        for component in relative.split('/') {
            let native_component = OsStrPathCodec.encode(component).map_err(|error| {
                FsError::with_source(
                    FsErrorKind::InvalidPath,
                    operation,
                    "canonical rooted path component cannot be decoded losslessly",
                    error,
                )
                .with_path(path.clone())
                .with_provider(LocalFileSystem::provider_id())
            })?;
            let native_component: &std::ffi::OsStr = native_component.as_ref();
            if Path::new(native_component)
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(FsError::invalid_path(
                    operation,
                    "canonical rooted path component introduces a native boundary",
                )
                .with_path(path.clone())
                .with_provider(LocalFileSystem::provider_id()));
            }
            native.push(native_component);
        }
        rooted::Path::new(native).map_err(|error| {
            FsError::from_io(error, operation)
                .with_path(path.clone())
                .with_provider(LocalFileSystem::provider_id())
        })
    }

    /// Maps descriptor-relative metadata into provider-neutral fixed fields.
    pub(crate) fn map_metadata(metadata: rooted::Metadata) -> FileMetadata {
        let kind = match metadata.kind() {
            rooted::EntryKind::File => FileKind::File,
            rooted::EntryKind::Directory => FileKind::Directory,
            rooted::EntryKind::Symlink => FileKind::Symlink,
            rooted::EntryKind::Other => {
                FileKind::Other("native-special".to_owned())
            }
        };
        let mut result = FileMetadata::new(kind);
        result.len = Some(metadata.size());
        result.accessed_at = metadata.accessed_at();
        result.modified_at = metadata.modified_at();
        result.created_at = metadata.created_at();
        result
    }

    /// Maps rooted native errors into provider-aware filesystem errors.
    pub(crate) fn map_io_error(
        operation: FsOperation,
        path: &FsPath,
        error: io::Error,
    ) -> FsError {
        FsError::from_io(error, operation)
            .with_path(path.clone())
            .with_provider(LocalFileSystem::provider_id())
    }

    /// Creates missing parents of one rooted destination.
    fn create_parent(&self, path: &rooted::Path) -> io::Result<()> {
        let Some(parent) = path
            .as_path()
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        else {
            return Ok(());
        };
        let parent = rooted::Path::new(parent)?;
        self.root.create_dir_all(&parent)
    }

    /// Removes one rooted entry without following symbolic links.
    fn remove_entry(
        &self,
        path: &rooted::Path,
        recursive: bool,
    ) -> io::Result<()> {
        let metadata = self.root.symlink_metadata(path)?;
        if metadata.kind() != rooted::EntryKind::Directory {
            return self.root.remove_file(path);
        }
        if recursive {
            self.root.remove_tree(path)
        } else {
            self.root.remove_empty_dir(path)
        }
    }

    /// Reads destination metadata, returning `None` for a missing entry.
    fn optional_metadata(
        &self,
        path: &rooted::Path,
    ) -> io::Result<Option<rooted::Metadata>> {
        match self.root.symlink_metadata(path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl FileSystemProperties for RootedLocalFileSystem {
    fn info(&self) -> &FileSystemInfo {
        &self.info
    }

    fn capabilities(&self) -> FileSystemCapabilities {
        self.capabilities
    }

    fn limits(&self) -> &FileSystemLimits {
        &self.limits
    }
}

impl FileSystem for RootedLocalFileSystem {
    fn list(
        &self,
        path: &FsPath,
        options: ListOptions,
    ) -> FsResult<DirectoryStream> {
        validate_hierarchical_path(FsOperation::List, path)?;
        let session =
            RootedDirectoryStreamSession::capture(&self.root, path, options)?;
        Ok(DirectoryStream::new(session))
    }

    fn create_dir(
        &self,
        path: &FsPath,
        options: CreateDirOptions,
    ) -> FsResult<()> {
        if options.user_metadata.as_metadata().iter().next().is_some() {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::CreateDir,
                "rooted local directories do not support user metadata",
            )
            .with_path(path.clone())
            .with_provider(LocalFileSystem::provider_id()));
        }
        let relative = Self::relative_path(path, FsOperation::CreateDir)?;
        let result = match (options.recursive, options.exists_ok) {
            (false, false) => self.root.create_dir(&relative),
            (false, true) => self.root.ensure_dir(&relative),
            (true, true) => self.root.ensure_dir_all(&relative),
            (true, false) => match self.optional_metadata(&relative) {
                Ok(Some(_)) => Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "rooted create_dir destination already exists",
                )),
                Ok(None) => self.root.create_dir_all(&relative),
                Err(error) => Err(error),
            },
        };
        result.map_err(|error| {
            Self::map_io_error(FsOperation::CreateDir, path, error)
        })
    }

    fn delete(&self, path: &FsPath, options: DeleteOptions) -> FsResult<()> {
        options
            .validate_against(self.capabilities)
            .map_err(|error| {
                error
                    .with_path(path.clone())
                    .with_provider(LocalFileSystem::provider_id())
            })?;
        let relative = Self::relative_path(path, FsOperation::Delete)?;
        match self.remove_entry(&relative, options.recursive) {
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
                    .with_provider(LocalFileSystem::provider_id())
            })?;
        let source = Self::relative_path(from, FsOperation::Rename)?;
        let destination = Self::relative_path(to, FsOperation::Rename)?;
        let result = if options.overwrite {
            self.root.rename(&source, &destination)
        } else {
            self.root.rename_without_replacing(&source, &destination)
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
                    .with_provider(LocalFileSystem::provider_id())
            })?;
        if options.continue_on_error {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::Copy,
                "rooted copy does not support continue-on-error",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(LocalFileSystem::provider_id()));
        }
        if !matches!(
            options.preserve_metadata,
            MetadataPreservePolicy::None | MetadataPreservePolicy::Portable
        ) {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::Copy,
                "rooted copy supports only none or portable metadata preservation",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(LocalFileSystem::provider_id()));
        }
        if options.follow_symlinks {
            return Err(FsError::new(
                FsErrorKind::UnsupportedCapability,
                FsOperation::Copy,
                "rooted copy does not follow symbolic links",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(LocalFileSystem::provider_id())
            .with_required_capability(FileSystemCapability::Symlink));
        }
        let source = Self::relative_path(from, FsOperation::Copy)?;
        let destination = Self::relative_path(to, FsOperation::Copy)?;
        if source == destination {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::Copy,
                "rooted copy source and destination must differ",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(LocalFileSystem::provider_id()));
        }
        let metadata =
            self.root.symlink_metadata(&source).map_err(|error| {
                Self::map_io_error(FsOperation::Copy, from, error)
                    .with_target(to.clone())
            })?;
        let copy_tree = match (options.mode, metadata.kind()) {
            (CopyMode::Auto | CopyMode::File, rooted::EntryKind::File) => false,
            (CopyMode::Auto | CopyMode::Tree, rooted::EntryKind::Directory) => {
                true
            }
            (_, rooted::EntryKind::Symlink | rooted::EntryKind::Other) => {
                return Err(FsError::new(
                    FsErrorKind::UnsupportedCapability,
                    FsOperation::Copy,
                    "rooted copy supports only regular files and directories",
                )
                .with_path(from.clone())
                .with_target(to.clone())
                .with_provider(LocalFileSystem::provider_id()));
            }
            _ => {
                return Err(FsError::new(
                    FsErrorKind::InvalidOptions,
                    FsOperation::Copy,
                    "copy mode does not match the source entry type",
                )
                .with_path(from.clone())
                .with_target(to.clone())
                .with_provider(LocalFileSystem::provider_id()));
            }
        };
        if copy_tree && destination.as_path().starts_with(source.as_path()) {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::Copy,
                "rooted tree destination must not be inside the source",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(LocalFileSystem::provider_id()));
        }
        if !copy_tree
            && self
                .optional_metadata(&destination)
                .map_err(|error| {
                    Self::map_io_error(FsOperation::Copy, to, error)
                        .with_target(to.clone())
                })?
                .as_ref()
                .is_some_and(|destination_metadata| {
                    metadata.is_same_file(destination_metadata)
                })
        {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::Copy,
                "rooted copy source and destination identify the same file",
            )
            .with_path(from.clone())
            .with_target(to.clone())
            .with_provider(LocalFileSystem::provider_id()));
        }
        if options.create_parent {
            self.create_parent(&destination).map_err(|error| {
                Self::map_io_error(FsOperation::Copy, to, error)
                    .with_target(to.clone())
            })?;
        }
        let conflict = match options.conflict {
            CopyConflictPolicy::Fail => copy::ConflictPolicy::Fail,
            CopyConflictPolicy::Overwrite => copy::ConflictPolicy::Overwrite,
            CopyConflictPolicy::Skip => copy::ConflictPolicy::Skip,
        };
        let type_conflict = if options.conflict == CopyConflictPolicy::Overwrite
        {
            copy::TypeConflictPolicy::Replace
        } else {
            copy::TypeConflictPolicy::Fail
        };
        let mut rooted_options = copy::Options::new()
            .with_conflict(conflict)
            .with_type_conflict(type_conflict);
        if options.preserve_metadata == MetadataPreservePolicy::Portable {
            rooted_options = rooted_options.preserve_permissions();
        }
        let rooted_stats = self
            .root
            .copy(&source, &destination, rooted_options)
            .map_err(|error| {
                let error = io::Error::new(error.kind(), error);
                Self::map_io_error(FsOperation::Copy, from, error)
                    .with_target(to.clone())
            })?;
        let stats = CopyStats {
            files: rooted_stats.files(),
            directories: rooted_stats.directories(),
            bytes: rooted_stats.bytes(),
            overwritten: rooted_stats.overwritten(),
            skipped: rooted_stats.skipped(),
            ..CopyStats::default()
        };
        Ok(CopyOutcome::new(
            stats,
            CopyMethod::Local,
            AchievedAtomicity::NonAtomic,
        ))
    }

    fn stat(&self, path: &FsPath) -> FsResult<FileMetadata> {
        let metadata = if path.as_str() == "/" {
            self.root.metadata()
        } else {
            let relative = Self::relative_path(path, FsOperation::Stat)?;
            self.root.symlink_metadata(&relative)
        };
        metadata.map(Self::map_metadata).map_err(|error| {
            FsError::from_io(error, FsOperation::Stat)
                .with_path(path.clone())
                .with_provider(LocalFileSystem::provider_id())
        })
    }

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
                    .with_provider(LocalFileSystem::provider_id())
            })?;
        let relative = Self::relative_path(path, FsOperation::OpenReader)?;
        let file = self
            .root
            .open_reader(&relative, &read::OpenOptions::default())
            .map_err(|error| {
                FsError::from_io(error, FsOperation::OpenReader)
                    .with_path(path.clone())
                    .with_provider(LocalFileSystem::provider_id())
            })?;
        let location = FileLocation::new(self.info.id().clone(), path.clone());
        Ok(FileReader::new(file, OpenedFileInfo::new(location)))
    }

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
                    .with_provider(LocalFileSystem::provider_id())
            })?;
        if options.content_type.is_some()
            || options.user_metadata.as_metadata().iter().next().is_some()
            || options.checksum.is_some()
        {
            return Err(FsError::new(
                FsErrorKind::InvalidOptions,
                FsOperation::OpenWriter,
                "rooted local writes do not support content metadata or checksums",
            )
            .with_path(path.clone())
            .with_provider(LocalFileSystem::provider_id()));
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
            .with_provider(LocalFileSystem::provider_id())
            .with_required_capability(FileSystemCapability::AtomicReplace));
        }
        let relative = Self::relative_path(path, FsOperation::OpenWriter)?;
        let session = if options.disposition
            == WriteDisposition::CreateOrReplace
            && options.atomicity != AtomicityRequirement::NotRequired
        {
            let atomic_options = if options.create_parent {
                atomic::Options::new().with_parent()
            } else {
                atomic::Options::new()
            };
            let writer = self
                .root
                .begin_atomic_write_with_options(&relative, atomic_options)
                .map_err(|error| {
                    let kind = error.kind();
                    FsError::from_io(
                        std::io::Error::new(kind, error),
                        FsOperation::OpenWriter,
                    )
                    .with_path(path.clone())
                    .with_provider(LocalFileSystem::provider_id())
                })?;
            RootedFileWriteSession::atomic(writer, path.clone())
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
            let file = self
                .root
                .open_writer(&relative, &local_options)
                .map_err(|error| {
                    FsError::from_io(error, FsOperation::OpenWriter)
                        .with_path(path.clone())
                        .with_provider(LocalFileSystem::provider_id())
                })?;
            RootedFileWriteSession::direct(file, path.clone())
        };
        let location = FileLocation::new(self.info.id().clone(), path.clone());
        Ok(FileWriter::new(session, OpenedFileInfo::new(location)))
    }
}
