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

/// A direct rooted write session backed by an already-contained file handle.
#[derive(Debug)]
pub(crate) struct RootedFileWriteSession {
    file: Option<File>,
    path: FsPath,
    bytes_written: u64,
}

impl RootedFileWriteSession {
    /// Creates a direct rooted write session.
    pub(crate) fn new(file: File, path: FsPath) -> Self {
        Self {
            file: Some(file),
            path,
            bytes_written: 0,
        }
    }

    /// Builds the stream error returned after the handle is consumed.
    fn closed_error(&self) -> io::Error {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            FsError::new(
                FsErrorKind::InvalidState,
                FsOperation::Write,
                "rooted write session is already closed",
            )
            .with_path(self.path.clone())
            .with_provider("local-file"),
        )
    }
}

impl Write for RootedFileWriteSession {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let Some(file) = self.file.as_mut() else {
            return Err(self.closed_error());
        };
        let written = file.write(buffer)?;
        self.bytes_written = self.bytes_written.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Err(self.closed_error());
        };
        file.flush()
    }

    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        let Some(file) = self.file.as_mut() else {
            return Err(self.closed_error());
        };
        let written = file.write_vectored(buffers)?;
        self.bytes_written = self.bytes_written.saturating_add(written as u64);
        Ok(written)
    }
}

impl FileWriteSession for RootedFileWriteSession {
    fn commit(&mut self) -> Result<WriteOutcome, WriteFailure> {
        let Some(file) = self.file.as_mut() else {
            return Err(WriteFailure::new(
                FsError::new(
                    FsErrorKind::InvalidState,
                    FsOperation::CommitWriter,
                    "rooted write session is already closed",
                )
                .with_path(self.path.clone())
                .with_provider("local-file"),
                WriteFailureState::Published,
            ));
        };
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                WriteFailure::new(
                    FsError::from_io(error, FsOperation::CommitWriter)
                        .with_path(self.path.clone())
                        .with_provider("local-file"),
                    WriteFailureState::Published,
                )
            })?;
        self.file.take();
        let mut outcome = WriteOutcome::new(
            AchievedAtomicity::NonAtomic,
            PublicationMethod::Direct,
        );
        outcome.bytes_written = Some(self.bytes_written);
        Ok(outcome)
    }

    fn abort(&mut self) -> FsResult<()> {
        self.file.take();
        Ok(())
    }
}
