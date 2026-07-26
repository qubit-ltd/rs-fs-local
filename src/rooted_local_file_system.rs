// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative local filesystem adapter.

use std::path::{
    Component,
    Path,
    PathBuf,
};

use qubit_fs::{
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
    OpenedFileInfo,
    OsStrPathCodec,
    PathSemantics,
    ReadOptions,
    WriteDisposition,
    WriteOptions,
};
use qubit_local_files::{
    read,
    rooted,
    write,
};

use crate::internal::RootedFileWriteSession;

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
    /// Descriptor-relative roots are currently available only on Unix targets.
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
        let info =
            FileSystemInfo::new(id, "local-file", PathSemantics::Hierarchical);
        Ok(Self {
            root,
            info,
            capabilities: FileSystemCapabilities::default()
                .with(FileSystemCapability::Read)
                .with(FileSystemCapability::Write)
                .with(FileSystemCapability::Append),
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
    fn relative_path(
        path: &FsPath,
        operation: FsOperation,
    ) -> FsResult<rooted::Path> {
        let relative = path.as_str().strip_prefix('/').ok_or_else(|| {
            FsError::invalid_path(
                operation,
                "rooted local filesystem paths must be absolute",
            )
            .with_path(path.clone())
            .with_provider("local-file")
        })?;
        if relative.is_empty() {
            return Err(FsError::invalid_path(
                operation,
                "rooted local filesystem root cannot be opened as a file",
            )
            .with_path(path.clone())
            .with_provider("local-file"));
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
                .with_provider("local-file")
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
                .with_provider("local-file"));
            }
            native.push(native_component);
        }
        rooted::Path::new(native).map_err(|error| {
            FsError::from_io(error, operation)
                .with_path(path.clone())
                .with_provider("local-file")
        })
    }

    /// Maps descriptor-relative metadata into provider-neutral fixed fields.
    fn map_metadata(metadata: rooted::Metadata) -> FileMetadata {
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
        result
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
                .with_provider("local-file")
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
                error.with_path(path.clone()).with_provider("local-file")
            })?;
        let relative = Self::relative_path(path, FsOperation::OpenReader)?;
        let file = self
            .root
            .open_reader(&relative, &read::OpenOptions::default())
            .map_err(|error| {
                FsError::from_io(error, FsOperation::OpenReader)
                    .with_path(path.clone())
                    .with_provider("local-file")
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
                error.with_path(path.clone()).with_provider("local-file")
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
            .with_provider("local-file"));
        }
        let relative = Self::relative_path(path, FsOperation::OpenWriter)?;
        let mode = match options.disposition {
            WriteDisposition::CreateNew => write::Mode::CreateNew,
            WriteDisposition::CreateOrReplace => write::Mode::CreateOrTruncate,
            WriteDisposition::Append => write::Mode::AppendExisting,
        };
        let local_options = if options.create_parent {
            write::OpenOptions::new(mode).with_parents()
        } else {
            write::OpenOptions::new(mode)
        };
        let file = self.root.open_writer(&relative, &local_options).map_err(
            |error| {
                FsError::from_io(error, FsOperation::OpenWriter)
                    .with_path(path.clone())
                    .with_provider("local-file")
            },
        )?;
        let location = FileLocation::new(self.info.id().clone(), path.clone());
        Ok(FileWriter::new(
            RootedFileWriteSession::new(file, path.clone()),
            OpenedFileInfo::new(location),
        ))
    }
}
