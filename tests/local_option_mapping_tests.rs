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
