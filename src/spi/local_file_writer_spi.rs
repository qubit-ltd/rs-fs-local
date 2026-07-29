// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Stateful writer adapter delegated to `qubit-local-files`.

use std::io::{
    Result as IoResult,
    Write,
};

use qubit_fs::spi::{
    FileWriterSpi,
    SpiWriteFailure,
};
use qubit_fs::{
    AchievedAtomicity,
    FsError,
    FsErrorKind,
    FsOperation,
    FsResult,
    PublicationMethod,
    WriteFailureState,
    WriteOutcome,
};
use qubit_io::Output;
use qubit_local_files as native_files;

pub(crate) struct LocalFileWriterSpi {
    writer: Option<native_files::LocalFileWriter>,
}

impl LocalFileWriterSpi {
    pub(crate) const fn new(writer: native_files::LocalFileWriter) -> Self {
        Self {
            writer: Some(writer),
        }
    }
    fn writer_mut(&mut self) -> IoResult<&mut native_files::LocalFileWriter> {
        self.writer
            .as_mut()
            .ok_or_else(|| std::io::Error::other("writer session is terminal"))
    }
}

impl Output for LocalFileWriterSpi {
    type Item = u8;
    unsafe fn write_unchecked(
        &mut self,
        input: &[u8],
        index: usize,
        count: usize,
    ) -> IoResult<usize> {
        Write::write(self.writer_mut()?, &input[index..index + count])
    }
    fn flush(&mut self) -> IoResult<()> {
        Write::flush(self.writer_mut()?)
    }
}

impl FileWriterSpi for LocalFileWriterSpi {
    fn commit(&mut self) -> Result<WriteOutcome, SpiWriteFailure> {
        let Some(writer) = self.writer.take() else {
            return Err(SpiWriteFailure::new(
                FsError::new(
                    FsErrorKind::InvalidState,
                    FsOperation::CommitWriter,
                    "writer session is terminal",
                ),
                WriteFailureState::NotPublished,
            ));
        };
        match writer.commit() {
            Ok(outcome) => {
                let mut result = WriteOutcome::new(
                    if outcome.atomic() {
                        AchievedAtomicity::Atomic
                    } else {
                        AchievedAtomicity::NonAtomic
                    },
                    if outcome.atomic() {
                        PublicationMethod::AtomicRename
                    } else {
                        PublicationMethod::Direct
                    },
                );
                result.bytes_written = Some(outcome.bytes_written());
                Ok(result)
            }
            Err(error) => {
                let (native, state, retained) = error.into_parts();
                self.writer = retained;
                let state = match state {
                    native_files::LocalWriterState::NotPublished
                        if self.writer.is_some() =>
                    {
                        WriteFailureState::RetryableNotPublished
                    }
                    native_files::LocalWriterState::NotPublished => {
                        WriteFailureState::NotPublished
                    }
                    native_files::LocalWriterState::Published => {
                        WriteFailureState::Published
                    }
                    _ => WriteFailureState::Indeterminate,
                };
                Err(SpiWriteFailure::new(
                    FsError::with_source(
                        FsErrorKind::Io,
                        FsOperation::CommitWriter,
                        "native writer commit failed",
                        native,
                    ),
                    state,
                ))
            }
        }
    }
    fn abort(&mut self) -> FsResult<()> {
        let Some(writer) = self.writer.take() else {
            return Ok(());
        };
        writer.abort().map(|_| ()).map_err(|error| {
            FsError::with_source(
                FsErrorKind::Io,
                FsOperation::AbortWriter,
                "native writer abort failed",
                error,
            )
        })
    }
}
