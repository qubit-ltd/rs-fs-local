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

use std::io::Result as IoResult;
use std::io::Write;

use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::error::FsResult;
use qubit_fs::metadata::AchievedAtomicity;
use qubit_fs::metadata::PublicationMethod;
use qubit_fs::metadata::WriteOutcome;
use qubit_fs::spi::FileWriterSpi;
use qubit_fs::spi::SpiWriteFailure;
use qubit_fs::write::WriteAbortOutcome;
use qubit_fs::write::WriteFailureState;
use qubit_io::Output;
use qubit_local_files as native_files;

use super::error_mapper;

/// Adapts one native writer and enforces terminal commit or abort state.
#[must_use]
pub(crate) struct LocalFileWriterSpi {
    /// Native writer retained until commit, abort, or an unrecoverable
    /// failure.
    writer: Option<native_files::LocalFileWriter>,
    /// Confirmed terminal publication state after an unrecoverable commit.
    terminal_abort_outcome: Option<WriteAbortOutcome>,
    /// Provider identity attached to lifecycle errors.
    provider_id: String,
}

impl LocalFileWriterSpi {
    /// Wraps an open native writer session.
    ///
    /// # Parameters
    ///
    /// - `writer`: Native writer owned by the new adapter.
    ///
    /// # Returns
    ///
    /// An active facade writer session.
    #[inline(always)]
    pub(crate) fn new(
        writer: native_files::LocalFileWriter,
        provider_id: String,
    ) -> Self {
        Self {
            writer: Some(writer),
            terminal_abort_outcome: None,
            provider_id,
        }
    }

    /// Borrows the active native writer for output operations.
    ///
    /// # Returns
    ///
    /// A mutable native writer while this session remains active.
    ///
    /// # Errors
    ///
    /// Returns an `Other` I/O error after the session becomes terminal.
    #[inline]
    fn writer_mut(&mut self) -> IoResult<&mut native_files::LocalFileWriter> {
        match self.writer.as_mut() {
            Some(writer) => Ok(writer),
            None => Err(std::io::Error::other("writer session is terminal")),
        }
    }
}

impl Output for LocalFileWriterSpi {
    type Item = u8;

    /// Writes a caller-validated byte range to the native writer.
    ///
    /// # Parameters
    ///
    /// - `input`: Source byte slice.
    /// - `index`: Starting offset of the validated range.
    /// - `count`: Number of bytes in the validated range.
    ///
    /// # Returns
    ///
    /// The number of bytes accepted by the native writer.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the session is terminal or native output
    /// fails.
    ///
    /// # Panics
    ///
    /// Panics if `index + count` falls outside `input`; callers satisfying the
    /// safety contract prevent this condition.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index..index + count` is a valid range
    /// within `input`, as required by [`Output::write_unchecked`].
    #[inline(always)]
    unsafe fn write_unchecked(
        &mut self,
        input: &[u8],
        index: usize,
        count: usize,
    ) -> IoResult<usize> {
        Write::write(self.writer_mut()?, &input[index..index + count])
    }

    /// Flushes buffered bytes through the active native writer.
    ///
    /// # Returns
    ///
    /// `Ok(())` after native buffers are flushed.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the session is terminal or the native flush
    /// fails.
    #[inline(always)]
    fn flush(&mut self) -> IoResult<()> {
        Write::flush(self.writer_mut()?)
    }
}

impl FileWriterSpi for LocalFileWriterSpi {
    /// Publishes the writer and records portable publication guarantees.
    ///
    /// # Returns
    ///
    /// The achieved atomicity, publication method, and written byte count.
    ///
    /// # Errors
    ///
    /// Returns `InvalidState` when the session is already terminal. Native
    /// commit failures retain their recovery state and preserve a retryable
    /// writer when the native backend provides one.
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
                    match outcome.publication_method() {
                        native_files::outcome::LocalWritePublicationMethod::AtomicRename => {
                            PublicationMethod::AtomicRename
                        }
                        native_files::outcome::LocalWritePublicationMethod::DirectAppend => {
                            PublicationMethod::Direct
                        }
                        _ => {
                            if outcome.atomic() {
                                PublicationMethod::AtomicRename
                            } else {
                                PublicationMethod::Direct
                            }
                        }
                    },
                );
                result =
                    result.with_bytes_written(outcome.bytes_written() as u64);
                Ok(result)
            }
            Err(error) => {
                let (native, state, retained) = error.into_parts();
                self.writer = retained;
                let state = write_failure_state(state, self.writer.is_some());
                if self.writer.is_none() {
                    self.terminal_abort_outcome =
                        Some(abort_outcome_from_failure(state));
                }
                Err(SpiWriteFailure::new(
                    error_mapper::map_without_path(
                        native,
                        FsOperation::CommitWriter,
                        "native writer commit failed",
                        &self.provider_id,
                    ),
                    state,
                ))
            }
        }
    }

    /// Aborts an active writer and makes the session terminal.
    ///
    /// # Returns
    ///
    /// The provider-confirmed destination state after native cleanup.
    ///
    /// # Errors
    ///
    /// Returns a mapped native abort error if cleanup of an active writer
    /// fails.
    fn abort(&mut self) -> FsResult<WriteAbortOutcome> {
        let Some(writer) = self.writer.as_mut() else {
            if let Some(outcome) = self.terminal_abort_outcome.take() {
                return Ok(outcome);
            }
            return Err(FsError::new(
                FsErrorKind::InvalidState,
                FsOperation::AbortWriter,
                "writer session is terminal",
            ));
        };
        match writer.abort() {
            Ok(outcome) => {
                let outcome = abort_outcome(outcome.failure_state());
                self.writer = None;
                Ok(outcome)
            }
            Err(error) => Err(abort_error(error, &self.provider_id)),
        }
    }
}

/// Converts a terminal portable commit state into its abort outcome.
#[inline]
fn abort_outcome_from_failure(state: WriteFailureState) -> WriteAbortOutcome {
    match state {
        WriteFailureState::RetryableNotPublished
        | WriteFailureState::NotPublished => WriteAbortOutcome::NotPublished,
        WriteFailureState::Published => WriteAbortOutcome::Published,
        WriteFailureState::Indeterminate => WriteAbortOutcome::Indeterminate,
    }
}

/// Converts native abort publication certainty to the portable outcome.
#[inline]
fn abort_outcome(
    state: Option<native_files::outcome::LocalWriteFailureState>,
) -> WriteAbortOutcome {
    match state {
        None
        | Some(native_files::outcome::LocalWriteFailureState::NotPublished) => {
            WriteAbortOutcome::NotPublished
        }
        Some(native_files::outcome::LocalWriteFailureState::Published) => {
            WriteAbortOutcome::Published
        }
        Some(native_files::outcome::LocalWriteFailureState::Indeterminate) => {
            WriteAbortOutcome::Indeterminate
        }
    }
}

/// Converts native commit state and retention into portable recovery state.
///
/// # Parameters
///
/// - `state`: Native publication state after commit failure.
/// - `retained`: Whether the adapter still owns a retryable native writer.
///
/// # Returns
///
/// The most precise portable writer failure state supported by both values.
#[inline]
fn write_failure_state(
    state: native_files::outcome::LocalWriteFailureState,
    retained: bool,
) -> WriteFailureState {
    match state {
        native_files::outcome::LocalWriteFailureState::NotPublished
            if retained =>
        {
            WriteFailureState::RetryableNotPublished
        }
        native_files::outcome::LocalWriteFailureState::NotPublished => {
            WriteFailureState::NotPublished
        }
        native_files::outcome::LocalWriteFailureState::Published => {
            WriteFailureState::Published
        }
        native_files::outcome::LocalWriteFailureState::Indeterminate => {
            WriteFailureState::Indeterminate
        }
    }
}

/// Maps a native abort failure without inventing logical path context.
///
/// # Parameters
///
/// - `error`: Native abort failure.
///
/// # Returns
///
/// A facade abort error with local provider context.
#[inline(always)]
fn abort_error(
    error: native_files::LocalFileError,
    provider_id: &str,
) -> FsError {
    error_mapper::map_without_path(
        error,
        FsOperation::AbortWriter,
        "native writer abort failed",
        provider_id,
    )
}
