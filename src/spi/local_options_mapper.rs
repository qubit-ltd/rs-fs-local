// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow all -- provider behavior is covered through facade
// contract tests.
//! Conversion of resolved core options to native local-files options.

use qubit_fs::spi::{
    ResolvedCreateDirectoryOptions,
    ResolvedDeleteOptions,
    ResolvedListOptions,
    ResolvedReadOptions,
    ResolvedRenameOptions,
    ResolvedWriteOptions,
};
use qubit_fs::{
    AtomicityRequirement,
    CopyConflictPolicy,
    CopyMode,
    DurabilityRequirement,
    FsError,
    FsErrorKind,
    FsOperation,
    MetadataPreservePolicy,
    WriteDisposition,
    WritePrecondition,
};
use qubit_local_files as native_files;

/// Creates native read options for resolved facade options.
///
/// # Parameters
///
/// - `_`: Resolved read options; the native backend needs no additional
///   configuration for the currently supported fields.
///
/// # Returns
///
/// Default native local read options.
#[inline(always)]
pub(crate) fn read(_: &ResolvedReadOptions) -> native_files::LocalReadOptions {
    native_files::LocalReadOptions::new()
}

/// Converts resolved listing options to native walker options.
///
/// # Parameters
///
/// - `options`: Resolved facade listing options.
///
/// # Returns
///
/// Native options that recurse when requested directly or when facade prefix
/// filtering requires descendant discovery.
///
/// # Errors
///
/// This conversion currently accepts every resolved listing shape; the
/// `Result` preserves the adapter conversion contract for future options.
#[inline]
pub(crate) fn list(
    options: &ResolvedListOptions,
) -> Result<native_files::LocalListOptions, FsError> {
    let mut native = native_files::LocalListOptions::new();
    if options.options().recursive || options.options().prefix.is_some() {
        native = native.with_recursive();
    }
    if options.options().follow_symlinks {
        native = native.with_follow_symlinks();
    }
    Ok(native)
}

/// Converts resolved writer options to native publication options.
///
/// # Parameters
///
/// - `options`: Resolved facade writer options.
///
/// # Returns
///
/// Native writer mode, atomicity, and parent-creation policy.
///
/// # Errors
///
/// Returns `RequirementNotMet` when the request contains a precondition,
/// content type, user metadata, or checksum that the native backend cannot
/// express.
pub(crate) fn write(
    options: &ResolvedWriteOptions,
) -> Result<native_files::LocalWriteOptions, FsError> {
    let options = options.options();
    let mode = match options.disposition {
        WriteDisposition::CreateNew => native_files::LocalWriteMode::CreateNew,
        WriteDisposition::CreateOrReplace => {
            native_files::LocalWriteMode::CreateOrReplace
        }
        WriteDisposition::Append => native_files::LocalWriteMode::Append,
    };
    let mut native = native_files::LocalWriteOptions::new(mode)
        .with_atomicity(atomicity(options.atomicity));
    if options.create_parent {
        native = native.with_parent();
    }
    if options.precondition != WritePrecondition::None
        || options.content_type.is_some()
        || !options.user_metadata.is_empty()
        || options.checksum.is_some()
    {
        return Err(unsupported(FsOperation::OpenWriter));
    }
    Ok(native)
}

/// Converts resolved directory-creation options to native options.
///
/// # Parameters
///
/// - `options`: Resolved facade directory-creation options.
///
/// # Returns
///
/// Native recursion and existing-directory policy.
///
/// # Errors
///
/// Returns `RequirementNotMet` when user metadata is requested because the
/// native backend cannot persist it.
#[inline]
pub(crate) fn create_directory(
    options: &ResolvedCreateDirectoryOptions,
) -> Result<native_files::LocalCreateDirectoryOptions, FsError> {
    if !options.options().user_metadata.is_empty() {
        return Err(unsupported(FsOperation::CreateDir));
    }
    let mut native = native_files::LocalCreateDirectoryOptions::new();
    if options.options().recursive {
        native = native.with_recursive();
    }
    if options.options().exists_ok {
        native = native.with_exists_ok();
    }
    Ok(native)
}

/// Converts resolved deletion options to native options.
///
/// # Parameters
///
/// - `options`: Resolved facade deletion options.
///
/// # Returns
///
/// Native recursion and missing-entry policy.
#[inline]
pub(crate) fn delete(
    options: &ResolvedDeleteOptions,
) -> native_files::LocalDeleteOptions {
    let mut native = native_files::LocalDeleteOptions::new();
    if options.options().recursive {
        native = native.with_recursive();
    }
    if options.options().missing_ok {
        native = native.with_missing_ok();
    }
    native
}

/// Converts resolved rename options to native options.
///
/// # Parameters
///
/// - `options`: Resolved facade rename options.
///
/// # Returns
///
/// Native overwrite and atomicity policy.
#[inline]
pub(crate) fn rename(
    options: &ResolvedRenameOptions,
) -> native_files::LocalRenameOptions {
    let mut native = native_files::LocalRenameOptions::new()
        .with_atomicity(atomicity(options.options().atomicity));
    if options.options().overwrite {
        native = native.with_overwrite();
    }
    native
}

/// Converts resolved copy options to native copy behavior.
///
/// # Parameters
///
/// - `options`: Resolved facade copy options.
///
/// # Returns
///
/// Native conflict, metadata, symlink, atomicity, durability, recursion, and
/// parent-creation policy.
///
/// # Errors
///
/// Returns `RequirementNotMet` for continue-on-error, required server-side
/// execution, or metadata preservation beyond portable permissions.
pub(crate) fn copy(
    options: &qubit_fs::spi::ResolvedCopyOptions,
) -> Result<native_files::LocalCopyOptions, FsError> {
    let options = options.options();
    if options.continue_on_error
        || options.server_side == qubit_fs::ServerSidePreference::Require
    {
        return Err(unsupported(FsOperation::Copy));
    }
    let mut native = native_files::LocalCopyOptions::new()
        .with_conflict(copy_conflict(options.conflict))
        .with_metadata_preservation(metadata_preservation(
            options.preserve_metadata,
        )?)
        .with_symlink_policy(symlink_policy(options.follow_symlinks))
        .with_atomicity(atomicity(options.atomicity))
        .with_durability(durability(options.durability));
    native = match options.mode {
        CopyMode::File => native.with_file_source(),
        CopyMode::Tree => native.with_tree_source(),
        CopyMode::Auto => native,
    };
    if options.conflict == CopyConflictPolicy::Overwrite {
        native = native.with_type_conflict(
            native_files::LocalCopyTypeConflictPolicy::Replace,
        );
    } else if options.conflict == CopyConflictPolicy::Skip {
        native = native.with_type_conflict(
            native_files::LocalCopyTypeConflictPolicy::Skip,
        );
    }
    if options.create_parent {
        native = native.with_parent();
    }
    Ok(native)
}

/// Converts portable copy conflict policy to native policy.
///
/// # Parameters
///
/// - `value`: Portable conflict policy to translate.
///
/// # Returns
///
/// The equivalent native conflict policy.
#[inline]
fn copy_conflict(
    value: CopyConflictPolicy,
) -> native_files::LocalCopyConflictPolicy {
    match value {
        CopyConflictPolicy::Fail => native_files::LocalCopyConflictPolicy::Fail,
        CopyConflictPolicy::Overwrite => {
            native_files::LocalCopyConflictPolicy::Overwrite
        }
        CopyConflictPolicy::Skip => native_files::LocalCopyConflictPolicy::Skip,
    }
}

/// Converts portable metadata preservation to native policy.
///
/// # Parameters
///
/// - `value`: Portable metadata policy to translate.
///
/// # Returns
///
/// No preservation or native permission preservation.
///
/// # Errors
///
/// Returns `RequirementNotMet` for metadata categories unavailable from the
/// native backend.
#[inline]
fn metadata_preservation(
    value: MetadataPreservePolicy,
) -> Result<native_files::LocalMetadataPreservePolicy, FsError> {
    match value {
        MetadataPreservePolicy::None => {
            Ok(native_files::LocalMetadataPreservePolicy::None)
        }
        MetadataPreservePolicy::Portable => {
            Ok(native_files::LocalMetadataPreservePolicy::Permissions)
        }
        MetadataPreservePolicy::UserMetadata
        | MetadataPreservePolicy::ProviderNative
        | MetadataPreservePolicy::All => Err(unsupported(FsOperation::Copy)),
    }
}

/// Converts the facade symlink flag to native copy policy.
///
/// # Parameters
///
/// - `follow`: Whether the copy request follows symbolic links.
///
/// # Returns
///
/// Native follow or reject behavior.
#[inline]
const fn symlink_policy(follow: bool) -> native_files::LocalSymlinkPolicy {
    if follow {
        native_files::LocalSymlinkPolicy::Follow
    } else {
        native_files::LocalSymlinkPolicy::Reject
    }
}

/// Converts facade atomicity requirements to native requirements.
///
/// # Parameters
///
/// - `value`: Portable atomicity requirement to translate.
///
/// # Returns
///
/// The equivalent native atomicity requirement.
#[inline]
const fn atomicity(
    value: AtomicityRequirement,
) -> native_files::LocalAtomicityRequirement {
    match value {
        AtomicityRequirement::Required => {
            native_files::LocalAtomicityRequirement::Required
        }
        AtomicityRequirement::Preferred => {
            native_files::LocalAtomicityRequirement::Preferred
        }
        AtomicityRequirement::NotRequired => {
            native_files::LocalAtomicityRequirement::NotRequired
        }
    }
}

/// Converts facade durability requirements to native requirements.
///
/// # Parameters
///
/// - `value`: Portable durability requirement to translate.
///
/// # Returns
///
/// The equivalent native durability requirement.
#[inline]
const fn durability(
    value: DurabilityRequirement,
) -> native_files::LocalDurabilityRequirement {
    match value {
        DurabilityRequirement::Required => {
            native_files::LocalDurabilityRequirement::Required
        }
        DurabilityRequirement::Preferred => {
            native_files::LocalDurabilityRequirement::Preferred
        }
        DurabilityRequirement::NotRequired => {
            native_files::LocalDurabilityRequirement::NotRequired
        }
    }
}

/// Builds an error for a facade option that the native backend cannot express.
///
/// # Parameters
///
/// - `operation`: Facade operation whose requirement cannot be met.
///
/// # Returns
///
/// A `RequirementNotMet` error scoped to `operation`.
#[inline(always)]
fn unsupported(operation: FsOperation) -> FsError {
    FsError::new(
        FsErrorKind::RequirementNotMet,
        operation,
        "local adapter cannot express requested option",
    )
}
