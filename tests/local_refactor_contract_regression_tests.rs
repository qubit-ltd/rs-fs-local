// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use qubit_fs::copy::CopyOptions;
use qubit_fs::error::FsErrorKind;
use qubit_fs::metadata::DurabilityRequirement;
use qubit_fs::path::Path;
use qubit_fs::write::WriteDisposition;
use qubit_fs::write::WriteFailureState;
use qubit_fs::write::WriteOptions;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;
use qubit_io::Output;
use qubit_local_files::test_support::install_test_fault;

#[cfg(unix)]
#[test]
fn test_host_required_writer_preserves_published_state_after_parent_sync_failure() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let target = host_path_to_logical(&root.path().join("one/two/payload")).expect("target should be representable");
    let filesystem =
        LocalFileSystems::host(LocalResourcePolicy::unbounded()).expect("host filesystem should construct");
    let mut writer = filesystem
        .open_writer(
            &target,
            WriteOptions::default()
                .with_disposition(WriteDisposition::CreateNew)
                .with_create_parent(true)
                .with_durability(DurabilityRequirement::Required),
        )
        .expect("writer should open");
    Output::write_fully(&mut writer, b"payload").expect("writer should accept payload");
    let _fault = install_test_fault("atomic-writer-created-parent-sync").expect("fault controller should install");

    let failure = writer
        .commit()
        .expect_err("ancestor synchronization failure should be visible");
    assert_eq!(WriteFailureState::Published, failure.state());
    assert_eq!(FsErrorKind::Io, failure.error().kind());
    assert_eq!(
        b"payload",
        std::fs::read(root.path().join("one/two/payload"))
            .expect("target should remain")
            .as_slice()
    );
}

#[test]
fn test_rooted_copy_budget_rejects_second_entry_without_partial_files() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    std::fs::create_dir(root.path().join("source")).expect("source should exist");
    for index in 0..100 {
        std::fs::write(root.path().join("source").join(format!("entry-{index}")), b"x")
            .expect("source entry should exist");
    }
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::unbounded())
        .expect("rooted filesystem should construct");
    let failure = filesystem
        .copy(
            &Path::parse("/source").expect("source path should parse"),
            &Path::parse("/target").expect("target path should parse"),
            CopyOptions::tree().with_max_entries(Some(1)),
        )
        .expect_err("second entry should exceed the copy budget");
    assert_eq!(FsErrorKind::ResourceLimitExceeded, failure.error().kind());
    assert_eq!(0, failure.partial_stats().files);
    assert_eq!(0, failure.partial_stats().bytes);
}
