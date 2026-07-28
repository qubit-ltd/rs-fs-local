//! Conversion of resolved core options to native local-files options.

use qubit_fs::spi::{
    ResolvedCreateDirectoryOptions, ResolvedDeleteOptions, ResolvedListOptions,
    ResolvedReadOptions, ResolvedRenameOptions, ResolvedWriteOptions,
};
use qubit_fs::{
    AtomicityRequirement, CopyConflictPolicy, CopyMode, DurabilityRequirement, FsError,
    FsErrorKind, FsOperation, MetadataPreservePolicy, WriteDisposition,
};
use qubit_local_files as native_files;

pub(crate) enum LocalOptionsMapper {}

impl LocalOptionsMapper {
    pub(crate) fn read(_: &ResolvedReadOptions) -> native_files::LocalReadOptions {
        native_files::LocalReadOptions::new()
    }
    pub(crate) fn list(options: &ResolvedListOptions) -> native_files::LocalListOptions {
        let mut native = native_files::LocalListOptions::new();
        if options.options().recursive {
            native = native.with_recursive();
        }
        if options.options().follow_symlinks {
            native = native.with_follow_symlinks();
        }
        native
    }
    pub(crate) fn write(
        options: &ResolvedWriteOptions,
    ) -> Result<native_files::LocalWriteOptions, FsError> {
        let options = options.options();
        let mode = match options.disposition {
            WriteDisposition::CreateNew => native_files::LocalWriteMode::CreateNew,
            WriteDisposition::CreateOrReplace => native_files::LocalWriteMode::CreateOrReplace,
            WriteDisposition::Append => native_files::LocalWriteMode::Append,
        };
        let mut native = native_files::LocalWriteOptions::new(mode)
            .with_atomicity(Self::atomicity(options.atomicity));
        if options.create_parent {
            native = native.with_parent();
        }
        Ok(native)
    }
    pub(crate) fn create_directory(
        options: &ResolvedCreateDirectoryOptions,
    ) -> native_files::LocalCreateDirectoryOptions {
        if options.options().recursive {
            native_files::LocalCreateDirectoryOptions::new().with_recursive()
        } else {
            native_files::LocalCreateDirectoryOptions::new()
        }
    }
    pub(crate) fn delete(options: &ResolvedDeleteOptions) -> native_files::LocalDeleteOptions {
        let mut native = native_files::LocalDeleteOptions::new();
        if options.options().recursive {
            native = native.with_recursive();
        }
        if options.options().missing_ok {
            native = native.with_missing_ok();
        }
        native
    }
    pub(crate) fn rename(options: &ResolvedRenameOptions) -> native_files::LocalRenameOptions {
        let mut native = native_files::LocalRenameOptions::new()
            .with_atomicity(Self::atomicity(options.options().atomicity));
        if options.options().overwrite {
            native = native.with_overwrite();
        }
        native
    }
    pub(crate) fn copy(
        options: &qubit_fs::spi::ResolvedCopyOptions,
    ) -> Result<native_files::LocalCopyOptions, FsError> {
        let options = options.options();
        if options.create_parent
            || options.continue_on_error
            || options.server_side != qubit_fs::ServerSidePreference::Disable
        {
            return Err(Self::unsupported(FsOperation::Copy));
        }
        let mut native = native_files::LocalCopyOptions::new()
            .with_conflict(match options.conflict {
                CopyConflictPolicy::Fail => native_files::LocalCopyConflictPolicy::Fail,
                CopyConflictPolicy::Overwrite => native_files::LocalCopyConflictPolicy::Overwrite,
                CopyConflictPolicy::Skip => native_files::LocalCopyConflictPolicy::Skip,
            })
            .with_metadata_preservation(match options.preserve_metadata {
                MetadataPreservePolicy::None => native_files::LocalMetadataPreservePolicy::None,
                _ => native_files::LocalMetadataPreservePolicy::Permissions,
            })
            .with_symlink_policy(if options.follow_symlinks {
                native_files::LocalSymlinkPolicy::Follow
            } else {
                native_files::LocalSymlinkPolicy::Reject
            })
            .with_atomicity(Self::atomicity(options.atomicity))
            .with_durability(Self::durability(options.durability));
        if options.mode == CopyMode::Tree {
            native = native.with_recursive();
        }
        Ok(native)
    }
    fn atomicity(value: AtomicityRequirement) -> native_files::LocalAtomicityRequirement {
        match value {
            AtomicityRequirement::Required => native_files::LocalAtomicityRequirement::Required,
            AtomicityRequirement::Preferred => native_files::LocalAtomicityRequirement::Preferred,
            AtomicityRequirement::NotRequired => {
                native_files::LocalAtomicityRequirement::NotRequired
            }
        }
    }
    fn durability(value: DurabilityRequirement) -> native_files::LocalDurabilityRequirement {
        match value {
            DurabilityRequirement::Required => native_files::LocalDurabilityRequirement::Required,
            DurabilityRequirement::Preferred => native_files::LocalDurabilityRequirement::Preferred,
            DurabilityRequirement::NotRequired => {
                native_files::LocalDurabilityRequirement::NotRequired
            }
        }
    }
    fn unsupported(operation: FsOperation) -> FsError {
        FsError::new(
            FsErrorKind::RequirementNotMet,
            operation,
            "local adapter cannot express requested option",
        )
    }
}
