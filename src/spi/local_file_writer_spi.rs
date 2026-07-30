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

use super::error_mapper::LocalFileErrorMapper;

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
        match self.writer.as_mut() {
            Some(writer) => Ok(writer),
            None => Err(std::io::Error::other("writer session is terminal")),
        }
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
                let state = write_failure_state(state, self.writer.is_some());
                Err(SpiWriteFailure::new(
                    LocalFileErrorMapper::map_without_path(
                        native,
                        FsOperation::CommitWriter,
                        "native writer commit failed",
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
        match writer.abort() {
            Ok(_) => Ok(()),
            Err(error) => Err(abort_error(error)),
        }
    }
}

fn write_failure_state(
    state: native_files::LocalWriterState,
    retained: bool,
) -> WriteFailureState {
    match state {
        native_files::LocalWriterState::NotPublished if retained => {
            WriteFailureState::RetryableNotPublished
        }
        native_files::LocalWriterState::NotPublished => {
            WriteFailureState::NotPublished
        }
        native_files::LocalWriterState::Published => {
            WriteFailureState::Published
        }
        _ => WriteFailureState::Indeterminate,
    }
}

fn abort_error(error: native_files::LocalFileError) -> FsError {
    LocalFileErrorMapper::map_without_path(
        error,
        FsOperation::AbortWriter,
        "native writer abort failed",
    )
}

#[cfg(test)]
mod tests {
    use qubit_fs::{
        FsErrorKind,
        spi::FileWriterSpi,
    };
    use qubit_io::Output;
    use qubit_local_files::{
        LocalFileError,
        LocalFileErrorKind,
        LocalFileOperation,
        LocalFileSystem,
        LocalWriteMode,
        LocalWriteOptions,
        LocalWriterState,
    };

    use super::{
        LocalFileWriterSpi,
        abort_error,
        write_failure_state,
    };

    #[test]
    fn classifies_every_native_commit_failure_state() {
        assert_eq!(
            qubit_fs::WriteFailureState::RetryableNotPublished,
            write_failure_state(LocalWriterState::NotPublished, true)
        );
        assert_eq!(
            qubit_fs::WriteFailureState::NotPublished,
            write_failure_state(LocalWriterState::NotPublished, false)
        );
        assert_eq!(
            qubit_fs::WriteFailureState::Published,
            write_failure_state(LocalWriterState::Published, false)
        );
        assert_eq!(
            qubit_fs::WriteFailureState::Indeterminate,
            write_failure_state(LocalWriterState::Indeterminate, false)
        );
    }

    #[test]
    fn maps_native_abort_error() {
        let error = abort_error(LocalFileError::new(
            LocalFileErrorKind::Io,
            LocalFileOperation::Abort,
        ));
        assert_eq!(FsErrorKind::Io, error.kind());
    }

    #[test]
    fn writes_commits_and_reports_a_terminal_commit() {
        let directory =
            tempfile::tempdir().expect("test directory must be created");
        let target = directory.path().join("published.txt");
        let native = LocalFileSystem::open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("native writer must open");
        let mut writer = LocalFileWriterSpi::new(native);
        Output::write_fully(&mut writer, b"payload")
            .expect("adapter writer must accept bytes");
        writer.flush().expect("adapter writer must flush");
        let outcome = writer.commit().ok().unwrap();
        assert_eq!(Some(7), outcome.bytes_written);
        let failure =
            writer.commit().expect_err("a committed writer is terminal");
        assert_eq!(FsErrorKind::InvalidState, failure.error().kind());
        writer.abort().expect("abort after commit is idempotent");
        assert_eq!(
            b"payload".to_vec(),
            std::fs::read(target).expect("output must exist")
        );
    }

    #[test]
    fn aborts_an_open_writer_and_rejects_further_output() {
        let directory =
            tempfile::tempdir().expect("test directory must be created");
        let target = directory.path().join("aborted.txt");
        let native = LocalFileSystem::open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("native writer must open");
        let mut writer = LocalFileWriterSpi::new(native);
        writer.abort().expect("open writer must abort");
        assert!(Output::write_fully(&mut writer, b"payload").is_err());
        assert!(!target.exists());
    }

    #[test]
    fn commit_failure_retains_the_writer_for_cleanup() {
        let directory =
            tempfile::tempdir().expect("test directory must be created");
        let target = directory.path().join("destination-directory");
        let native = LocalFileSystem::open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("staged writer must open before publication");
        let mut writer = LocalFileWriterSpi::new(native);
        Output::write_fully(&mut writer, b"payload")
            .expect("writer must accept bytes before publication");
        std::fs::create_dir(&target)
            .expect("destination directory must be created after opening");
        let failure = writer.commit().err().unwrap();
        assert_eq!(FsErrorKind::AlreadyExists, failure.error().kind());
        writer.abort().expect("retained writer must be abortable");
    }

    #[test]
    fn appending_reports_direct_nonatomic_publication() {
        let directory =
            tempfile::tempdir().expect("test directory must be created");
        let target = directory.path().join("append.txt");
        std::fs::write(&target, b"before")
            .expect("initial content must be written");
        let native = LocalFileSystem::open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .expect("append writer must open");
        let mut writer = LocalFileWriterSpi::new(native);
        Output::write_fully(&mut writer, b"+after")
            .expect("append writer must accept bytes");
        let outcome = writer.commit().ok().unwrap();
        assert_eq!(qubit_fs::AchievedAtomicity::NonAtomic, outcome.atomicity);
        assert_eq!(qubit_fs::PublicationMethod::Direct, outcome.method);
        assert_eq!(
            b"before+after".to_vec(),
            std::fs::read(target).expect("appended content must exist")
        );
    }
}
