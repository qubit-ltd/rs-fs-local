// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Direct descriptor-relative write sessions.

use std::fs::File;
use std::io::{
    self,
    IoSlice,
    Write,
};

use qubit_fs::{
    AchievedAtomicity,
    FileWriteSession,
    FsError,
    FsErrorKind,
    FsOperation,
    FsPath,
    FsResult,
    PublicationMethod,
    WriteFailure,
    WriteFailureState,
    WriteOutcome,
};
use qubit_local_files::{
    atomic,
    rooted,
};

use crate::LocalFileSystem;

/// Direct or staged write session beneath an opened filesystem root.
#[derive(Debug)]
pub(crate) enum RootedFileWriteSession {
    /// Writer that modifies the contained destination directly.
    Direct {
        /// Active contained file handle.
        file: Option<File>,
        /// Canonical destination path used for error context.
        path: FsPath,
        /// Number of bytes accepted by the file.
        bytes_written: u64,
    },
    /// Writer that atomically publishes beneath the opened root.
    Atomic {
        /// Active descriptor-relative staging writer.
        writer: Option<rooted::Writer>,
        /// Canonical destination path used for error context.
        path: FsPath,
        /// Number of bytes accepted by the staging writer.
        bytes_written: u64,
        /// Destination state retained after a failed consuming commit.
        terminal_state: Option<atomic::DestinationState>,
    },
}

impl RootedFileWriteSession {
    /// Creates a direct rooted write session.
    #[inline(always)]
    pub(crate) fn direct(file: File, path: FsPath) -> Self {
        Self::Direct {
            file: Some(file),
            path,
            bytes_written: 0,
        }
    }

    /// Creates an atomic rooted write session.
    #[inline(always)]
    pub(crate) fn atomic(writer: rooted::Writer, path: FsPath) -> Self {
        Self::Atomic {
            writer: Some(writer),
            path,
            bytes_written: 0,
            terminal_state: None,
        }
    }

    /// Maps a rooted atomic error into provider filesystem context.
    fn map_atomic_error(
        path: &FsPath,
        operation: FsOperation,
        error: atomic::Error,
    ) -> FsError {
        let kind = error.kind();
        FsError::from_io(io::Error::new(kind, error), operation)
            .with_path(path.clone())
            .with_provider(LocalFileSystem::provider_id())
    }

    /// Builds the stream error returned after the active writer is consumed.
    fn closed_error(path: &FsPath) -> io::Error {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            FsError::new(
                FsErrorKind::InvalidState,
                FsOperation::Write,
                "rooted write session is already closed",
            )
            .with_path(path.clone())
            .with_provider(LocalFileSystem::provider_id()),
        )
    }
}

impl Write for RootedFileWriteSession {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let (writer, bytes_written): (&mut dyn Write, &mut u64) = match self {
            Self::Direct {
                file: Some(file),
                bytes_written,
                ..
            } => (file, bytes_written),
            Self::Atomic {
                writer: Some(writer),
                bytes_written,
                ..
            } => (writer, bytes_written),
            Self::Direct { path, .. } | Self::Atomic { path, .. } => {
                return Err(Self::closed_error(path));
            }
        };
        let written = writer.write(buffer)?;
        *bytes_written = bytes_written.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Direct {
                file: Some(file), ..
            } => file.flush(),
            Self::Atomic {
                writer: Some(writer),
                ..
            } => writer.flush(),
            Self::Direct { path, .. } | Self::Atomic { path, .. } => {
                Err(Self::closed_error(path))
            }
        }
    }

    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        let (writer, bytes_written): (&mut dyn Write, &mut u64) = match self {
            Self::Direct {
                file: Some(file),
                bytes_written,
                ..
            } => (file, bytes_written),
            Self::Atomic {
                writer: Some(writer),
                bytes_written,
                ..
            } => (writer, bytes_written),
            Self::Direct { path, .. } | Self::Atomic { path, .. } => {
                return Err(Self::closed_error(path));
            }
        };
        let written = writer.write_vectored(buffers)?;
        *bytes_written = bytes_written.saturating_add(written as u64);
        Ok(written)
    }
}

impl FileWriteSession for RootedFileWriteSession {
    fn commit(&mut self) -> Result<WriteOutcome, WriteFailure> {
        match self {
            Self::Direct {
                file,
                path,
                bytes_written,
            } => {
                let Some(active_file) = file.as_mut() else {
                    return Err(WriteFailure::new(
                        FsError::new(
                            FsErrorKind::InvalidState,
                            FsOperation::CommitWriter,
                            "rooted direct write session was already consumed",
                        )
                        .with_path(path.clone())
                        .with_provider(LocalFileSystem::provider_id()),
                        WriteFailureState::NotPublished,
                    ));
                };
                active_file
                    .flush()
                    .and_then(|()| active_file.sync_all())
                    .map_err(|error| {
                        WriteFailure::new(
                            FsError::from_io(error, FsOperation::CommitWriter)
                                .with_path(path.clone())
                                .with_provider(LocalFileSystem::provider_id()),
                            WriteFailureState::Retryable,
                        )
                    })?;
                file.take();
                let mut outcome = WriteOutcome::new(
                    AchievedAtomicity::NonAtomic,
                    PublicationMethod::Direct,
                );
                outcome.bytes_written = Some(*bytes_written);
                Ok(outcome)
            }
            Self::Atomic {
                writer,
                path,
                bytes_written,
                terminal_state,
            } => {
                let Some(active_writer) = writer.take() else {
                    return Err(WriteFailure::new(
                        FsError::new(
                            FsErrorKind::InvalidState,
                            FsOperation::CommitWriter,
                            "rooted atomic write session was already consumed",
                        )
                        .with_path(path.clone())
                        .with_provider(LocalFileSystem::provider_id()),
                        WriteFailureState::Indeterminate,
                    ));
                };
                match active_writer.commit_recoverable() {
                    Ok(()) => {
                        let mut outcome = WriteOutcome::new(
                            AchievedAtomicity::Atomic,
                            PublicationMethod::AtomicRename,
                        );
                        outcome.bytes_written = Some(*bytes_written);
                        Ok(outcome)
                    }
                    Err(commit_error) => {
                        let (error, retained_writer) =
                            commit_error.into_parts();
                        let destination_state = error.destination_state();
                        *terminal_state = Some(destination_state);
                        let is_retryable = retained_writer.is_some()
                            && destination_state
                                == atomic::DestinationState::Unchanged;
                        *writer = retained_writer;
                        let state = match destination_state {
                            atomic::DestinationState::Unchanged
                                if is_retryable =>
                            {
                                WriteFailureState::Retryable
                            }
                            atomic::DestinationState::Unchanged
                            | atomic::DestinationState::Missing => {
                                WriteFailureState::NotPublished
                            }
                            atomic::DestinationState::Replaced => {
                                WriteFailureState::Published
                            }
                            atomic::DestinationState::Indeterminate => {
                                WriteFailureState::Indeterminate
                            }
                            _ => WriteFailureState::Indeterminate,
                        };
                        Err(WriteFailure::new(
                            Self::map_atomic_error(
                                path,
                                FsOperation::CommitWriter,
                                error,
                            ),
                            state,
                        ))
                    }
                }
            }
        }
    }

    fn abort(&mut self) -> FsResult<()> {
        match self {
            Self::Direct { file, .. } => {
                file.take();
                Ok(())
            }
            Self::Atomic {
                writer,
                path,
                terminal_state,
                ..
            } => {
                if let Some(writer) = writer.take() {
                    return writer.abort().map_err(|error| {
                        Self::map_atomic_error(
                            path,
                            FsOperation::AbortWriter,
                            error,
                        )
                    });
                }
                if matches!(
                    terminal_state,
                    Some(
                        atomic::DestinationState::Missing
                            | atomic::DestinationState::Indeterminate
                    )
                ) {
                    return Err(FsError::new(
                        FsErrorKind::Indeterminate,
                        FsOperation::AbortWriter,
                        "rooted atomic staging cleanup cannot be confirmed",
                    )
                    .with_path(path.clone())
                    .with_provider(LocalFileSystem::provider_id()));
                }
                Ok(())
            }
        }
    }
}
