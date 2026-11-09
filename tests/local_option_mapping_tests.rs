// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::num::NonZeroUsize;
use std::time::Duration;

use qubit_fs::copy::CopyFailureState;
use qubit_fs::copy::CopyOptions;
use qubit_fs::copy::MetadataPreservePolicy;
use qubit_fs::copy::ServerSidePreference;
use qubit_fs::directory::CreateDirectoryOptions;
use qubit_fs::directory::ListFilter;
use qubit_fs::directory::ListOptions;
use qubit_fs::directory::ListScope;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::metadata::Checksum;
use qubit_fs::metadata::ChecksumAlgorithm;
#[cfg(unix)]
use qubit_fs::metadata::DurabilityRequirement;
use qubit_fs::metadata::UserMetadata;
use qubit_fs::path::Path;
use qubit_fs::write::WriteOptions;
use qubit_fs::write::WritePrecondition;
use qubit_fs_local::LocalCopyResourceLimits;
use qubit_fs_local::LocalDeleteResourceLimits;
use qubit_fs_local::LocalDirectoryReopenPolicy;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalListResourceLimits;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;
use qubit_local_files::LocalFileError;

fn path(value: &str) -> Path {
    Path::parse(value).expect("test path should be valid")
}

fn bounded_policy() -> LocalResourcePolicy {
    let list =
        LocalListResourceLimits::new(8, 64, 255, 4, Duration::from_secs(5)).expect("list policy should be valid");
    let copy =
        LocalCopyResourceLimits::new(8, 64, 1024, 4, Duration::from_secs(5)).expect("copy policy should be valid");
    let delete = LocalDeleteResourceLimits::new(8, 64, 1024, Duration::from_secs(5));
    LocalResourcePolicy::bounded(list, copy, delete)
}

#[test]
fn test_caller_list_entry_budget_is_enforced_with_local_policy() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    std::fs::write(root.path().join("one"), b"1").expect("first fixture should be written");
    std::fs::write(root.path().join("two"), b"2").expect("second fixture should be written");
    let filesystem =
        LocalFileSystems::rooted(root.path(), bounded_policy()).expect("rooted filesystem should construct");
    let mut stream = filesystem
        .list(
            &ListScope::Path((Path::root()).clone()),
            ListOptions::default().with_max_entries(Some(1)),
        )
        .expect("listing should open");

    stream
        .next_entry()
        .expect("first entry should fit")
        .expect("first entry should exist");
    let error = stream
        .next_entry()
        .expect_err("second entry should exceed the caller budget");
    assert_eq!(FsErrorKind::ResourceLimitExceeded, error.kind());
}

#[test]
fn test_caller_copy_byte_budget_is_forwarded_to_native_copy() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    std::fs::write(root.path().join("source"), b"four").expect("source should be written");
    let filesystem =
        LocalFileSystems::rooted(root.path(), bounded_policy()).expect("rooted filesystem should construct");

    let failure = filesystem
        .copy(
            &path("/source"),
            &path("/target"),
            CopyOptions::file().with_max_bytes(Some(2)),
        )
        .expect_err("copy should exceed the caller byte budget");
    assert_eq!(FsErrorKind::ResourceLimitExceeded, failure.error().kind());
}

#[cfg(unix)]
#[test]
fn test_required_write_durability_is_forwarded_and_reported() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");

    let outcome = filesystem
        .write_all(
            &path("/durable"),
            b"payload",
            WriteOptions::default().with_durability(DurabilityRequirement::Required),
        )
        .expect("durability-required local write should succeed");
    assert!(outcome.durable());
}

#[test]
fn test_local_execution_policy_builders_preserve_all_native_controls() {
    let timeout = Duration::from_millis(250);
    let attempts = NonZeroUsize::new(32).expect("positive attempt count");
    let policy = LocalResourcePolicy::unbounded()
        .with_open_retry_timeout(Some(timeout))
        .with_temp_max_attempts(Some(attempts))
        .with_directory_reopen_policy(LocalDirectoryReopenPolicy::Fail);

    assert_eq!(Some(timeout), policy.open_retry_timeout());
    assert_eq!(Some(attempts), policy.temp_max_attempts());
    assert_eq!(LocalDirectoryReopenPolicy::Fail, policy.directory_reopen_policy());
}

/// Deletion requests cannot discard the provider's recursive resource ceiling.
#[test]
fn test_recursive_delete_cannot_bypass_provider_resource_limits() {
    use qubit_fs::directory::DeleteOptions;
    use qubit_fs_local::LocalDeleteResourceLimits;

    for rooted in [false, true] {
        let root = tempfile::tempdir().expect("fixture should exist");
        let tree = root.path().join("tree");
        std::fs::create_dir(&tree).expect("tree should exist");
        std::fs::write(tree.join("child"), b"data").expect("child should exist");
        let policy = LocalResourcePolicy::unbounded().with_delete_limits(Some(LocalDeleteResourceLimits::new(
            8,
            1,
            4096,
            Duration::from_secs(60),
        )));
        let filesystem = if rooted {
            LocalFileSystems::rooted(root.path(), policy)
        } else {
            LocalFileSystems::host(policy)
        }
        .expect("filesystem should open");
        let operand = if rooted {
            path("/tree")
        } else {
            host_path_to_logical(&tree).expect("Host path should convert")
        };
        let error = filesystem
            .delete_directory(&operand, DeleteOptions::default().with_recursive(true))
            .expect_err("request must retain the provider ceiling");
        assert_eq!(FsErrorKind::ResourceLimitExceeded, error.kind());
        assert!(tree.join("child").exists());
    }
}

/// Neither omitted nor looser request limits can widen a provider's ceiling.
#[test]
fn test_list_and_copy_requests_cannot_relax_provider_ceilings() {
    for requested in [None, Some(100)] {
        let root = tempfile::tempdir().expect("fixture should exist");
        std::fs::write(root.path().join("one"), b"ab").expect("first fixture should exist");
        std::fs::write(root.path().join("two"), b"cd").expect("second fixture should exist");
        let policy = LocalResourcePolicy::bounded(
            LocalListResourceLimits::new(8, 1, 4096, 4, Duration::from_secs(60))
                .expect("listing limits should be valid"),
            LocalCopyResourceLimits::new(8, 10, 1, 4, Duration::from_secs(60)).expect("copy limits should be valid"),
            LocalDeleteResourceLimits::new(8, 10, 4096, Duration::from_secs(60)),
        );
        let filesystem = LocalFileSystems::rooted(root.path(), policy).expect("filesystem should open");
        let mut stream = filesystem
            .list(
                &ListScope::Path((Path::root()).clone()),
                ListOptions::default().with_max_entries(requested),
            )
            .expect("stream should open");
        assert!(stream.next_entry().expect("first entry fits").is_some());
        assert_eq!(
            FsErrorKind::ResourceLimitExceeded,
            stream
                .next_entry()
                .expect_err("second entry exceeds provider ceiling")
                .kind()
        );
        let failure = filesystem
            .copy(
                &path("/one"),
                &path("/target"),
                CopyOptions::file().with_max_bytes(requested.map(|value| value as u64)),
            )
            .expect_err("copy must retain provider byte ceiling");
        assert_eq!(FsErrorKind::ResourceLimitExceeded, failure.error().kind());
        assert!(!root.path().join("target").exists());
    }
}

/// A zero request budget fails during native provider open with list context.
#[test]
fn test_list_deadline_request_zero() {
    let root = tempfile::tempdir().expect("fixture should exist");
    std::fs::write(root.path().join("entry"), b"data").expect("fixture entry should exist");
    let filesystem = LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("filesystem should open");
    let scope_path = host_path_to_logical(root.path()).expect("fixture path should convert");
    let error = filesystem
        .list(
            &ListScope::Path(scope_path),
            ListOptions::default().with_deadline(Some(Duration::ZERO)),
        )
        .expect_err("zero request deadline should fail while the provider opens the walker");
    assert_eq!(FsErrorKind::Timeout, error.kind());
    assert_eq!(qubit_fs::error::FsOperation::List, error.operation());
}

/// A zero provider budget is enforced while the native walker opens.
#[test]
fn test_list_deadline_provider_zero() {
    assert_provider_zero_deadline(None);
}

/// A caller's positive budget cannot relax a zero provider deadline.
#[test]
fn test_list_deadline_provider_cannot_be_relaxed() {
    assert_provider_zero_deadline(Some(Duration::from_secs(5)));
}

fn assert_provider_zero_deadline(request_deadline: Option<Duration>) {
    let root = tempfile::tempdir().expect("fixture should exist");
    std::fs::write(root.path().join("entry"), b"data").expect("fixture entry should exist");
    let limits =
        LocalListResourceLimits::new(8, 64, 255, 4, Duration::ZERO).expect("zero provider deadline should be valid");
    let policy = LocalResourcePolicy::unbounded().with_list_limits(Some(limits));
    let filesystem = LocalFileSystems::host(policy).expect("filesystem should open");
    let scope_path = host_path_to_logical(root.path()).expect("fixture path should convert");
    let error = filesystem
        .list(
            &ListScope::Path(scope_path),
            ListOptions::default().with_deadline(request_deadline),
        )
        .expect_err("zero provider deadline should fail while the provider opens the walker");
    assert_eq!(FsErrorKind::Timeout, error.kind());
    assert_eq!(qubit_fs::error::FsOperation::List, error.operation());
}

fn assert_refusal(error: &qubit_fs::error::FsError, operation: FsOperation, reason: &str) {
    assert_eq!(FsErrorKind::RequirementNotMet, error.kind());
    assert_eq!(operation, error.operation());
    assert!(error.to_string().contains(reason), "unexpected refusal: {error}");
}

#[test]
fn test_unsupported_write_options_report_specific_reason_and_preserve_target() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let target = root.path().join("existing");
    std::fs::write(&target, b"original").expect("target should be seeded");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");
    let metadata = UserMetadata::new()
        .with("key", "value")
        .expect("metadata should be valid");
    let cases = [
        (
            WriteOptions::default().with_precondition(WritePrecondition::IfAbsent),
            "conditional writes",
        ),
        (
            WriteOptions::default().with_content_type(Some("text/plain".to_owned())),
            "write content type",
        ),
        (
            WriteOptions::default().with_user_metadata(metadata),
            "write user metadata",
        ),
        (
            WriteOptions::default().with_checksum(Some(Checksum::new(ChecksumAlgorithm::Sha256, "00"))),
            "write checksum",
        ),
    ];
    for (options, reason) in cases {
        let error = filesystem
            .write_all(&path("/existing"), b"replacement", options)
            .expect_err("unsupported write option should fail");
        assert_refusal(error.error(), FsOperation::OpenWriter, reason);
        assert_eq!(
            b"original",
            std::fs::read(&target).expect("target should remain").as_slice()
        );
    }
    let error = filesystem
        .write_all(
            &path("/new"),
            b"payload",
            WriteOptions::default()
                .with_content_type(Some("text/plain".to_owned()))
                .with_user_metadata(UserMetadata::new().with("key", "value").expect("valid metadata"))
                .with_checksum(Some(Checksum::new(ChecksumAlgorithm::Sha256, "00"))),
        )
        .expect_err("multiple unsupported options should fail");
    assert_refusal(error.error(), FsOperation::OpenWriter, "write content type");
    assert!(!root.path().join("new").exists());
}

#[test]
fn test_unsupported_copy_options_report_specific_reason_without_effects() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    std::fs::write(root.path().join("source"), b"source").expect("source should be seeded");
    std::fs::write(root.path().join("target"), b"original").expect("target should be seeded");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");
    for (options, reason) in [
        (CopyOptions::file().with_continue_on_error(true), "continue-on-error"),
        (
            CopyOptions::file().with_server_side(ServerSidePreference::Require),
            "server-side copy is required",
        ),
        (
            CopyOptions::file().with_preserve_metadata(MetadataPreservePolicy::UserMetadata),
            "user metadata",
        ),
        (
            CopyOptions::file().with_preserve_metadata(MetadataPreservePolicy::ProviderNative),
            "provider-native metadata",
        ),
        (
            CopyOptions::file().with_preserve_metadata(MetadataPreservePolicy::All),
            "all metadata",
        ),
    ] {
        let failure = filesystem
            .copy(&path("/source"), &path("/target"), options)
            .expect_err("unsupported copy option should fail");
        assert_refusal(failure.error(), FsOperation::Copy, reason);
        assert_eq!(CopyFailureState::Unchanged, failure.state());
        assert_eq!(
            b"original",
            std::fs::read(root.path().join("target"))
                .expect("target should remain")
                .as_slice()
        );
        assert_eq!(
            b"source",
            std::fs::read(root.path().join("source"))
                .expect("source should remain")
                .as_slice()
        );
    }
    let failure = filesystem
        .copy(
            &path("/source"),
            &path("/missing-target"),
            CopyOptions::file()
                .with_continue_on_error(true)
                .with_preserve_metadata(MetadataPreservePolicy::All),
        )
        .expect_err("multiple unsupported options should fail");
    assert_refusal(failure.error(), FsOperation::Copy, "continue-on-error");
    assert_eq!(CopyFailureState::Unchanged, failure.state());
    assert!(!root.path().join("missing-target").exists());
}

#[test]
fn test_unsupported_list_and_directory_options_report_specific_reason() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    std::fs::write(root.path().join("entry"), b"data").expect("fixture entry should exist");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");
    let literal_prefix = filesystem
        .list(
            &ListScope::Path(path("/")),
            ListOptions::default().with_filter(Some(ListFilter::LiteralPrefix("ent".to_owned()))),
        )
        .expect_err("literal-prefix filter should be rejected");
    assert_eq!(FsErrorKind::InvalidOptions, literal_prefix.kind());
    assert_eq!(FsOperation::List, literal_prefix.operation());

    let metadata = UserMetadata::new()
        .with("key", "value")
        .expect("metadata should be valid");
    let directory = filesystem
        .create_directory(
            &path("/new-directory"),
            CreateDirectoryOptions::default().with_user_metadata(metadata),
        )
        .expect_err("directory user metadata should be rejected");
    assert_refusal(&directory, FsOperation::CreateDir, "directory user metadata");
    assert!(!root.path().join("new-directory").exists());
}

/// Partial recursive deletion retains portable budget classification and
/// effects.
#[test]
fn test_recursive_delete_preserves_budget_cause_and_partial_effect() {
    use std::error::Error;

    use qubit_fs::directory::DeleteOptions;
    use qubit_fs::error::FsEffectState;
    use qubit_fs_local::LocalDeleteResourceLimits;

    let root = tempfile::tempdir().expect("fixture should exist");
    for name in ["first", "second"] {
        let branch = root.path().join("tree").join(name);
        std::fs::create_dir_all(&branch).expect("branch should exist");
        std::fs::write(branch.join("payload"), b"data").expect("payload should exist");
    }
    let policy = LocalResourcePolicy::unbounded().with_delete_limits(Some(LocalDeleteResourceLimits::new(
        8,
        4,
        4096,
        Duration::from_secs(60),
    )));
    let filesystem = LocalFileSystems::rooted(root.path(), policy).expect("filesystem should open");
    let error = filesystem
        .delete_directory(&path("/tree"), DeleteOptions::default().with_recursive(true))
        .expect_err("second branch exceeds budget");
    assert_eq!(FsErrorKind::ResourceLimitExceeded, error.kind());
    assert_eq!(Some(FsEffectState::PartiallyApplied), error.effect_state());
    let native = error
        .source()
        .expect("native error should be retained")
        .downcast_ref::<LocalFileError>()
        .expect("native error should stay typed");
    assert_eq!(Some(4), native.resource_limit_error().map(|error| error.limit()));
}

/// An expired deletion deadline keeps both its category and partial effects.
#[test]
fn test_delete_deadline_preserves_timeout_and_partial_effects() {
    use qubit_fs::directory::DeleteOptions;
    use qubit_fs::error::FsEffectState;
    use qubit_fs_local::LocalDeleteResourceLimits;
    use qubit_local_files::test_support::install_test_fault;

    let directory = tempfile::tempdir().expect("fixture should exist");
    std::fs::create_dir(directory.path().join("tree")).expect("tree should exist");
    std::fs::write(directory.path().join("tree/child"), b"data").expect("child should exist");
    let policy = LocalResourcePolicy::unbounded().with_delete_limits(Some(LocalDeleteResourceLimits::new(
        10,
        10,
        1024,
        Duration::from_secs(60),
    )));
    let filesystem = LocalFileSystems::rooted(directory.path(), policy).expect("Rooted facade should open");
    let _fault = install_test_fault("local-delete-deadline-9").expect("deadline fault should install");
    let error = filesystem
        .delete_directory(&path("/tree"), DeleteOptions::default().with_recursive(true))
        .expect_err("deadline should expire after child deletion");
    assert_eq!(FsErrorKind::Timeout, error.kind());
    assert_eq!(Some(FsEffectState::PartiallyApplied), error.effect_state());
    assert!(directory.path().join("tree").exists());
    assert!(!directory.path().join("tree/child").exists());
}
