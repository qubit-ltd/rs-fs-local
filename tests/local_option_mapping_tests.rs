// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::num::NonZeroUsize;
use std::time::Duration;

use qubit_fs::copy::CopyOptions;
use qubit_fs::directory::ListOptions;
use qubit_fs::error::FsErrorKind;
use qubit_fs::metadata::DurabilityRequirement;
use qubit_fs::path::Path;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalCopyResourceLimits;
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
        LocalListResourceLimits::new(8, 64, 255, 4, Duration::from_secs(5))
            .expect("list policy should be valid");
    let copy =
        LocalCopyResourceLimits::new(8, 64, 1024, 4, Duration::from_secs(5))
            .expect("copy policy should be valid");
    LocalResourcePolicy::bounded(list, copy)
}

#[test]
fn caller_list_entry_budget_is_enforced_with_local_policy() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    std::fs::write(root.path().join("one"), b"1")
        .expect("first fixture should be written");
    std::fs::write(root.path().join("two"), b"2")
        .expect("second fixture should be written");
    let filesystem = LocalFileSystems::rooted(root.path(), bounded_policy())
        .expect("rooted filesystem should construct");
    let mut stream = filesystem
        .list(
            &Path::root(),
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
fn caller_copy_byte_budget_is_forwarded_to_native_copy() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    std::fs::write(root.path().join("source"), b"four")
        .expect("source should be written");
    let filesystem = LocalFileSystems::rooted(root.path(), bounded_policy())
        .expect("rooted filesystem should construct");

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
fn required_write_durability_is_forwarded_and_reported() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let filesystem =
        LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
            .expect("rooted filesystem should construct");

    let outcome = filesystem
        .write_all(
            &path("/durable"),
            b"payload",
            WriteOptions::default()
                .with_durability(DurabilityRequirement::Required),
        )
        .expect("durability-required local write should succeed");
    assert!(outcome.durable());
}

#[test]
fn local_execution_policy_builders_preserve_all_native_controls() {
    let timeout = Duration::from_millis(250);
    let attempts = NonZeroUsize::new(32).expect("positive attempt count");
    let policy = LocalResourcePolicy::unbounded()
        .with_open_retry_timeout(Some(timeout))
        .with_temp_max_attempts(Some(attempts))
        .with_directory_reopen_policy(LocalDirectoryReopenPolicy::Fail);

    assert_eq!(Some(timeout), policy.open_retry_timeout());
    assert_eq!(Some(attempts), policy.temp_max_attempts());
    assert_eq!(
        LocalDirectoryReopenPolicy::Fail,
        policy.directory_reopen_policy()
    );
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
        std::fs::write(tree.join("child"), b"data")
            .expect("child should exist");
        let policy = LocalResourcePolicy::unbounded().with_delete_limits(Some(
            LocalDeleteResourceLimits::new(8, 1, 4096, Duration::from_secs(60)),
        ));
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
            .delete_directory(
                &operand,
                DeleteOptions::default().with_recursive(true),
            )
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
        std::fs::write(root.path().join("one"), b"ab")
            .expect("first fixture should exist");
        std::fs::write(root.path().join("two"), b"cd")
            .expect("second fixture should exist");
        let policy = LocalResourcePolicy::bounded(
            LocalListResourceLimits::new(
                8,
                1,
                4096,
                4,
                Duration::from_secs(60),
            )
            .expect("listing limits should be valid"),
            LocalCopyResourceLimits::new(8, 10, 1, 4, Duration::from_secs(60))
                .expect("copy limits should be valid"),
        );
        let filesystem = LocalFileSystems::rooted(root.path(), policy)
            .expect("filesystem should open");
        let mut stream = filesystem
            .list(
                &Path::root(),
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
                CopyOptions::file()
                    .with_max_bytes(requested.map(|value| value as u64)),
            )
            .expect_err("copy must retain provider byte ceiling");
        assert_eq!(FsErrorKind::ResourceLimitExceeded, failure.error().kind());
        assert!(!root.path().join("target").exists());
    }
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
        std::fs::write(branch.join("payload"), b"data")
            .expect("payload should exist");
    }
    let policy = LocalResourcePolicy::unbounded().with_delete_limits(Some(
        LocalDeleteResourceLimits::new(8, 4, 4096, Duration::from_secs(60)),
    ));
    let filesystem = LocalFileSystems::rooted(root.path(), policy)
        .expect("filesystem should open");
    let error = filesystem
        .delete_directory(
            &path("/tree"),
            DeleteOptions::default().with_recursive(true),
        )
        .expect_err("second branch exceeds budget");
    assert_eq!(FsErrorKind::ResourceLimitExceeded, error.kind());
    assert_eq!(Some(FsEffectState::PartiallyApplied), error.effect_state());
    let native = error
        .source()
        .expect("native error should be retained")
        .downcast_ref::<LocalFileError>()
        .expect("native error should stay typed");
    assert_eq!(
        Some(4),
        native.resource_limit_error().map(|error| error.limit())
    );
}

/// An expired deletion deadline keeps both its category and partial effects.
#[test]
fn test_delete_deadline_preserves_timeout_and_partial_effects() {
    use qubit_fs::directory::DeleteOptions;
    use qubit_fs::error::FsEffectState;
    use qubit_fs_local::LocalDeleteResourceLimits;
    use qubit_local_files::test_support::install_test_fault;

    let directory = tempfile::tempdir().expect("fixture should exist");
    std::fs::create_dir(directory.path().join("tree"))
        .expect("tree should exist");
    std::fs::write(directory.path().join("tree/child"), b"data")
        .expect("child should exist");
    let policy = LocalResourcePolicy::unbounded().with_delete_limits(Some(
        LocalDeleteResourceLimits::new(10, 10, 1024, Duration::from_secs(60)),
    ));
    let filesystem = LocalFileSystems::rooted(directory.path(), policy)
        .expect("Rooted facade should open");
    let _fault = install_test_fault("local-delete-deadline-8")
        .expect("deadline fault should install");
    let error = filesystem
        .delete_directory(
            &path("/tree"),
            DeleteOptions::default().with_recursive(true),
        )
        .expect_err("deadline should expire after child deletion");
    assert_eq!(FsErrorKind::Timeout, error.kind());
    assert_eq!(Some(FsEffectState::PartiallyApplied), error.effect_state());
    assert!(directory.path().join("tree").exists());
    assert!(!directory.path().join("tree/child").exists());
}
