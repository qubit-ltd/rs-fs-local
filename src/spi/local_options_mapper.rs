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

use qubit_fs::copy::CopyConflictPolicy;
use qubit_fs::copy::CopyMode;
use qubit_fs::copy::MetadataPreservePolicy;
use qubit_fs::copy::ServerSidePreference;
use qubit_fs::error::FsError;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::metadata::AtomicityRequirement;
use qubit_fs::metadata::DurabilityRequirement;
use qubit_fs::metadata::SymlinkPolicy;
use qubit_fs::spi::ResolvedCopyOptions;
use qubit_fs::spi::ResolvedCreateDirectoryOptions;
use qubit_fs::spi::ResolvedDeleteOptions;
use qubit_fs::spi::ResolvedListOptions;
use qubit_fs::spi::ResolvedReadOptions;
use qubit_fs::spi::ResolvedRenameOptions;
use qubit_fs::spi::ResolvedWriteOptions;
use qubit_fs::write::WriteDisposition;
use qubit_fs::write::WritePrecondition;
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
    scope: native_files::LocalFileSystemScope,
    defaults: native_files::LocalListOptions,
) -> Result<native_files::LocalListOptions, FsError> {
    let mut native = defaults;
    if options.options().recursive() || options.options().prefix().is_some() {
        native = native.with_recursive();
    }
    if let Some(policy) = options.options().symlink_policy_override() {
        native = native.with_symlink_policy(native_symlink_policy(
            policy,
            scope,
            FsOperation::List,
        )?);
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
    let mode = match options.disposition() {
        WriteDisposition::CreateNew => native_files::LocalWriteMode::CreateNew,
        WriteDisposition::CreateOrReplace => {
            native_files::LocalWriteMode::CreateOrReplace
        }
        WriteDisposition::Append => native_files::LocalWriteMode::Append,
    };
    let mut native = native_files::LocalWriteOptions::new(mode)
        .with_atomicity(atomicity(options.atomicity()));
    if options.create_parent() {
        native = native.with_parent();
    }
    if *options.precondition() != WritePrecondition::None
        || options.content_type().is_some()
        || !options.user_metadata().is_empty()
        || options.checksum().is_some()
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
    if !options.options().user_metadata().is_empty() {
        return Err(unsupported(FsOperation::CreateDir));
    }
    let mut native = native_files::LocalCreateDirectoryOptions::new();
    if options.options().recursive() {
        native = native.with_recursive();
    }
    if options.options().exists_ok() {
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
    if options.options().recursive() {
        native = native.with_recursive();
    }
    if options.options().missing_ok() {
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
/// Native overwrite and durability policies. The native rename primitive is
/// always atomic, so the adapter does not forward the facade's redundant
/// atomicity preference.
#[inline]
pub(crate) fn rename(
    options: &ResolvedRenameOptions,
) -> native_files::LocalRenameOptions {
    let mut native = native_files::LocalRenameOptions::new();
    if options.options().overwrite() {
        native = native.with_overwrite();
    }
    native = native.with_durability(match options.options().durability() {
        DurabilityRequirement::Required => {
            native_files::LocalDurabilityRequirement::Required
        }
        DurabilityRequirement::Preferred => {
            native_files::LocalDurabilityRequirement::Preferred
        }
        DurabilityRequirement::NotRequired => {
            native_files::LocalDurabilityRequirement::NotRequired
        }
    });
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
    options: &ResolvedCopyOptions,
    scope: native_files::LocalFileSystemScope,
    defaults: native_files::LocalCopyOptions,
) -> Result<native_files::LocalCopyOptions, FsError> {
    let symlink_policy = options.symlink_policy();
    let options = options.options();
    if options.continue_on_error()
        || options.server_side() == ServerSidePreference::Require
    {
        return Err(unsupported(FsOperation::Copy));
    }
    let mut native = defaults
        .with_conflict(copy_conflict(options.conflict()))
        .with_metadata_preservation(metadata_preservation(
            options.preserve_metadata(),
        )?)
        .with_atomicity(atomicity(options.atomicity()))
        .with_durability(durability(options.durability()));
    if options.symlink_policy_override().is_some() {
        native = native.with_symlink_policy(native_symlink_policy(
            symlink_policy,
            scope,
            FsOperation::Copy,
        )?);
    }
    native = match options.mode() {
        CopyMode::File => native.with_file_source(),
        CopyMode::Tree => native.with_tree_source(),
        CopyMode::Auto => native,
    };
    if options.conflict() == CopyConflictPolicy::Overwrite {
        native = native.with_type_conflict(
            native_files::LocalCopyTypeConflictPolicy::Replace,
        );
    } else if options.conflict() == CopyConflictPolicy::Skip {
        native = native.with_type_conflict(
            native_files::LocalCopyTypeConflictPolicy::Skip,
        );
    }
    if options.create_parent() {
        native = native.with_parent();
    }
    Ok(native)
}

/// Maps an abstract operation override to the native scope-aware policy.
fn native_symlink_policy(
    policy: SymlinkPolicy,
    scope: native_files::LocalFileSystemScope,
    operation: FsOperation,
) -> Result<native_files::LocalSymlinkPolicy, FsError> {
    match policy {
        SymlinkPolicy::Reject => Ok(native_files::LocalSymlinkPolicy::Reject),
        SymlinkPolicy::FollowWithinFileSystem => Ok(match scope {
            native_files::LocalFileSystemScope::Host => {
                native_files::LocalSymlinkPolicy::FollowAcrossScope
            }
            native_files::LocalFileSystemScope::Rooted => {
                native_files::LocalSymlinkPolicy::FollowWithinScope
            }
        }),
        _ => Err(unsupported(operation)),
    }
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
