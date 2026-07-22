// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
//! Local synchronous file write session.

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
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriter,
    LocalFileWriter,
};

use crate::LocalFileSystem;

/// Direct or staged write session used by the local filesystem provider.
#[derive(Debug)]
pub(crate) enum LocalFileWriteSession {
    /// Writer that modifies the destination directly.
    Direct {
        /// Active native writer, removed when the session is aborted.
        writer: Option<LocalFileWriter>,
        /// Number of bytes accepted by the writer.
        bytes_written: u64,
        /// Canonical destination path used for error context.
        path: FsPath,
    },
    /// Writer that publishes through an atomic same-directory replacement.
    Atomic {
        /// Active staging writer, consumed by commit or abort.
        writer: Option<LocalAtomicWriter>,
        /// Number of bytes accepted by the staging writer.
        bytes_written: u64,
        /// Canonical destination path used for error context.
        path: FsPath,
        /// Destination state retained after a failed consuming commit.
        terminal_state: Option<LocalAtomicDestinationState>,
    },
}

impl LocalFileWriteSession {
    /// Creates a direct local write session.
    ///
    /// # Parameters
    ///
    /// * `writer` - Open native destination writer.
    /// * `path` - Canonical destination path used for error context.
    ///
    /// # Returns
    /// A session that publishes bytes directly to the destination.
    #[inline(always)]
    pub(crate) fn direct(writer: LocalFileWriter, path: FsPath) -> Self {
        Self::Direct {
            writer: Some(writer),
            bytes_written: 0,
            path,
        }
    }

    /// Creates an atomic local write session.
    ///
    /// # Parameters
    ///
    /// * `writer` - Open same-directory staging writer.
    /// * `path` - Canonical destination path used for error context.
    ///
    /// # Returns
    /// A session that publishes bytes through atomic replacement.
    #[inline(always)]
    pub(crate) fn atomic(writer: LocalAtomicWriter, path: FsPath) -> Self {
        Self::Atomic {
            writer: Some(writer),
            bytes_written: 0,
            path,
            terminal_state: None,
        }
    }

    /// Maps a local atomic error into a provider-contextual filesystem error.
    ///
    /// # Parameters
    ///
    /// * `path` - Canonical destination path.
    /// * `operation` - Provider-neutral operation that failed.
    /// * `error` - Structured native atomic-write failure.
    ///
    /// # Returns
    /// A filesystem error retaining the atomic failure as source context.
    fn map_atomic_error(
        path: &FsPath,
        operation: FsOperation,
        error: LocalAtomicWriteError,
    ) -> FsError {
        let kind = error.kind();
        FsError::from_io(io::Error::new(kind, error), operation)
            .with_path(path.clone())
            .with_provider(LocalFileSystem::provider_id())
    }

    /// Returns an invalid-state stream error after a consuming operation.
    ///
    /// # Parameters
    ///
    /// * `path` - Canonical destination path.
    ///
    /// # Returns
    /// A broken-pipe error retaining provider-neutral lifecycle context.
    fn closed_io_error(path: &FsPath) -> io::Error {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            FsError::new(
                FsErrorKind::InvalidState,
                FsOperation::Write,
                "local write session no longer accepts bytes",
            )
            .with_path(path.clone())
            .with_provider(LocalFileSystem::provider_id()),
        )
    }
}

impl Write for LocalFileWriteSession {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let (writer, bytes_written): (&mut dyn Write, &mut u64) = match self {
            Self::Direct {
                writer: Some(writer),
                bytes_written,
                ..
            } => (writer, bytes_written),
            Self::Atomic {
                writer: Some(writer),
                bytes_written,
                ..
            } => (writer, bytes_written),
            Self::Direct { path, .. } | Self::Atomic { path, .. } => {
                return Err(Self::closed_io_error(path));
            }
        };
        let written = writer.write(buffer)?;
        *bytes_written = bytes_written.saturating_add(written as u64);
        Ok(written)
    }

    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        let (writer, bytes_written): (&mut dyn Write, &mut u64) = match self {
            Self::Direct {
                writer: Some(writer),
                bytes_written,
                ..
            } => (writer, bytes_written),
            Self::Atomic {
                writer: Some(writer),
                bytes_written,
                ..
            } => (writer, bytes_written),
            Self::Direct { path, .. } | Self::Atomic { path, .. } => {
                return Err(Self::closed_io_error(path));
            }
        };
        let written = writer.write_vectored(buffers)?;
        *bytes_written = bytes_written.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Direct {
                writer: Some(writer),
                ..
            } => writer.flush(),
            Self::Atomic {
                writer: Some(writer),
                ..
            } => writer.flush(),
            Self::Direct { path, .. } | Self::Atomic { path, .. } => {
                Err(Self::closed_io_error(path))
            }
        }
    }
}

impl FileWriteSession for LocalFileWriteSession {
    fn commit(&mut self) -> Result<WriteOutcome, WriteFailure> {
        match self {
            Self::Direct {
                writer: Some(writer),
                bytes_written,
                path,
            } => {
                writer.sync_all().map_err(|error| {
                    WriteFailure::new(
                        FsError::from_io(error, FsOperation::CommitWriter)
                            .with_path(path.clone())
                            .with_provider(LocalFileSystem::provider_id()),
                        WriteFailureState::Retryable,
                    )
                })?;
                let mut outcome = WriteOutcome::new(
                    AchievedAtomicity::NonAtomic,
                    PublicationMethod::Direct,
                );
                outcome.bytes_written = Some(*bytes_written);
                Ok(outcome)
            }
            Self::Atomic {
                writer,
                bytes_written,
                path,
                terminal_state,
            } => {
                let Some(writer) = writer.take() else {
                    return Err(WriteFailure::new(
                        FsError::new(
                            FsErrorKind::InvalidState,
                            FsOperation::CommitWriter,
                            "atomic write session was already consumed",
                        )
                        .with_path(path.clone())
                        .with_provider(LocalFileSystem::provider_id()),
                        WriteFailureState::Indeterminate,
                    ));
                };
                match writer.commit() {
                    Ok(()) => {
                        let mut outcome = WriteOutcome::new(
                            AchievedAtomicity::Atomic,
                            PublicationMethod::AtomicRename,
                        );
                        outcome.bytes_written = Some(*bytes_written);
                        Ok(outcome)
                    }
                    Err(error) => {
                        let destination_state = error.destination_state();
                        *terminal_state = Some(destination_state);
                        let state = match destination_state {
                            LocalAtomicDestinationState::Unchanged
                            | LocalAtomicDestinationState::Missing => {
                                WriteFailureState::NotPublished
                            }
                            LocalAtomicDestinationState::Replaced => {
                                WriteFailureState::Published
                            }
                            LocalAtomicDestinationState::Indeterminate => {
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
            Self::Direct { path, .. } => Err(WriteFailure::new(
                FsError::new(
                    FsErrorKind::InvalidState,
                    FsOperation::CommitWriter,
                    "direct write session was already aborted",
                )
                .with_path(path.clone())
                .with_provider(LocalFileSystem::provider_id()),
                WriteFailureState::NotPublished,
            )),
        }
    }

    fn abort(&mut self) -> FsResult<()> {
        match self {
            Self::Direct { writer, .. } => {
                writer.take();
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
                        LocalAtomicDestinationState::Missing
                            | LocalAtomicDestinationState::Indeterminate
                    )
                ) {
                    return Err(FsError::new(
                        FsErrorKind::Indeterminate,
                        FsOperation::AbortWriter,
                        "atomic staging cleanup cannot be confirmed",
                    )
                    .with_path(path.clone())
                    .with_provider(LocalFileSystem::provider_id()));
                }
                Ok(())
            }
        }
    }
}
