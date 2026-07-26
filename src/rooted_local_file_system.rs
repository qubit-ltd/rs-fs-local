// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative local filesystem adapter.

use std::fs;
use std::path::Path;

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
    FsOperation,
    FsPath,
    FsResult,
    OpenedFileInfo,
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
    /// Opens a descriptor-relative filesystem rooted at `path`.
    ///
    /// # Errors
    /// Returns an I/O error when the root cannot be securely opened.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let root = rooted::Root::open(path)?;
        let id = FileSystemId::new("local-rooted")
            .expect("the static rooted filesystem ID must be valid");
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
        rooted::Path::new(relative).map_err(|error| {
            FsError::from_io(error, operation)
                .with_path(path.clone())
                .with_provider("local-file")
        })
    }

    /// Maps native metadata into the provider-neutral fixed fields.
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
        let path = Self::relative_path(path, FsOperation::Stat)?;
        let file = self
            .root
            .open_reader(&path, &read::OpenOptions::default())
            .map_err(|error| {
                FsError::from_io(error, FsOperation::Stat)
                    .with_provider("local-file")
            })?;
        file.metadata()
            .map(Self::map_metadata)
            .map_err(|error| FsError::from_io(error, FsOperation::Stat))
    }

    fn open_reader(
        &self,
        path: &FsPath,
        options: ReadOptions,
    ) -> FsResult<FileReader> {
        options.validate_against(self.capabilities)?;
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
        options.validate_against(self.capabilities)?;
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
