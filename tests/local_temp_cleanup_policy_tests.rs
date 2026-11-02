// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Contract tests for independently controlled temporary-directory cleanup.

use std::error::Error;
use std::time::Duration;

use qubit_fs::error::FsEffectState;
use qubit_fs::error::FsErrorKind;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::temp::TempOptions;
use qubit_fs::temp::TempResourceState;
use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::LocalTempCleanupControl;
use qubit_fs_local::LocalTempCleanupResourceLimits;
use qubit_fs_local::host_path_to_logical;
use qubit_local_files::error::LocalFileError;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::test_support::install_test_fault;

#[cfg(feature = "registry")]
static CONST_HOST_PROVIDER: qubit_fs_local::LocalFileSystemProvider =
    qubit_fs_local::LocalFileSystemProvider::host(LocalResourcePolicy::unbounded());

#[test]
fn test_rooted_cleanup_recovers_after_explicit_budget_increase() {
    let root = tempfile::tempdir().expect("root fixture");
    let control = LocalTempCleanupControl::new(Some(limits(0, 100)));
    let filesystem = LocalFileSystems::rooted_with_id_and_temp_cleanup(
        FileSystemId::new("cleanup-test").expect("filesystem id"),
        root.path(),
        LocalResourcePolicy::standard(),
        control.clone(),
    )
    .expect("rooted filesystem");
    let mut directory = filesystem
        .create_temp_directory(TempOptions::default())
        .expect("temporary directory");
    let native_directory = root.path().join(directory.path().as_str().trim_start_matches('/'));
    std::fs::write(native_directory.join("entry"), b"data").expect("temporary entry");

    let error = directory.cleanup().expect_err("zero entries block cleanup");
    assert_eq!(error.kind(), FsErrorKind::ResourceLimitExceeded);
    assert_eq!(directory.state(), TempResourceState::CleanupRequired);

    control.set_limits(Some(limits(8, 100)));
    directory.cleanup().expect("same handle recovers with larger budget");
    assert_eq!(directory.state(), TempResourceState::Cleaned);
}

#[test]
fn test_default_cleanup_limits_remain_unbounded() {
    let root = tempfile::tempdir().expect("root fixture");
    let filesystem = LocalFileSystems::rooted(root.path(), LocalResourcePolicy::standard()).expect("rooted filesystem");
    let mut directory = filesystem
        .create_temp_directory(TempOptions::default())
        .expect("temporary directory");
    let native_directory = root.path().join(directory.path().as_str().trim_start_matches('/'));
    std::fs::write(native_directory.join("entry"), b"data").expect("temporary entry");
    directory.cleanup().expect("default cleanup has no additional limits");
}

#[test]
fn test_host_cleanup_recovers_after_explicit_budget_increase() {
    let parent = tempfile::tempdir().expect("host parent fixture");
    let logical_parent = host_path_to_logical(parent.path()).expect("logical host parent");
    let control = LocalTempCleanupControl::new(Some(limits(8, 0)));
    let filesystem = LocalFileSystems::host_with_temp_cleanup(LocalResourcePolicy::standard(), control.clone())
        .expect("host filesystem");
    let mut directory = filesystem
        .create_temp_directory(TempOptions::default().with_parent(Some(logical_parent)))
        .expect("temporary directory");
    let sandbox = std::fs::read_dir(parent.path())
        .expect("read host parent")
        .next()
        .expect("temporary sandbox")
        .expect("sandbox entry")
        .path();
    let native_directory = std::fs::read_dir(sandbox)
        .expect("read temporary sandbox")
        .next()
        .expect("one generated temporary directory")
        .expect("temporary directory entry")
        .path();
    std::fs::write(native_directory.join("entry"), b"data").expect("temporary entry");

    let error = directory.cleanup().expect_err("zero entries block cleanup");
    assert_eq!(error.kind(), FsErrorKind::ResourceLimitExceeded);
    assert_eq!(directory.state(), TempResourceState::CleanupRequired);
    control.set_limits(Some(limits(8, 100)));
    directory
        .cleanup()
        .expect("same host handle recovers with larger budget");
}

#[test]
fn test_cleanup_budget_updates_after_directory_creation() {
    let root = tempfile::tempdir().expect("root fixture");
    let control = LocalTempCleanupControl::new(None);
    let filesystem = LocalFileSystems::rooted_with_id_and_temp_cleanup(
        FileSystemId::new("cleanup-adjust-after-create").expect("filesystem id"),
        root.path(),
        LocalResourcePolicy::standard(),
        control.clone(),
    )
    .expect("rooted filesystem");
    let mut directory = filesystem
        .create_temp_directory(TempOptions::default())
        .expect("temporary directory");
    let native_directory = root.path().join(directory.path().as_str().trim_start_matches('/'));
    std::fs::write(native_directory.join("entry"), b"data").expect("temporary entry");

    control.set_limits(Some(limits(8, 0)));
    let error = directory.cleanup().expect_err("updated zero budget blocks cleanup");
    assert_eq!(error.kind(), FsErrorKind::ResourceLimitExceeded);
    assert!(native_directory.join("entry").exists());

    control.set_limits(Some(limits(8, 100)));
    directory.cleanup().expect("updated budget permits recovery");
}

#[test]
fn test_depth_and_pending_path_budgets_block_then_recover() {
    for limits_value in [
        LocalTempCleanupResourceLimits::new(0, 100, 1 << 20, Duration::from_secs(30)),
        LocalTempCleanupResourceLimits::new(8, 100, 0, Duration::from_secs(30)),
        LocalTempCleanupResourceLimits::new(8, 100, 1 << 20, Duration::ZERO),
    ] {
        let root = tempfile::tempdir().expect("root fixture");
        let control = LocalTempCleanupControl::new(Some(limits_value));
        let filesystem = LocalFileSystems::rooted_with_id_and_temp_cleanup(
            FileSystemId::new("cleanup-budget-boundary").expect("filesystem id"),
            root.path(),
            LocalResourcePolicy::standard(),
            control.clone(),
        )
        .expect("rooted filesystem");
        let mut directory = filesystem
            .create_temp_directory(TempOptions::default())
            .expect("temporary directory");
        let native_directory = root.path().join(directory.path().as_str().trim_start_matches('/'));
        std::fs::write(native_directory.join("entry"), b"data").expect("temporary entry");

        let error = directory.cleanup().expect_err("cleanup budget blocks tree");
        assert!(matches!(
            error.kind(),
            FsErrorKind::ResourceLimitExceeded | FsErrorKind::Timeout
        ));
        assert_eq!(TempResourceState::CleanupRequired, directory.state());
        assert!(native_directory.join("entry").exists());

        control.set_limits(Some(limits(8, 100)));
        directory.cleanup().expect("larger budget recovers the same handle");
    }
}

#[test]
fn test_budget_change_does_not_restore_indeterminate_source_authority() {
    let root = tempfile::tempdir().expect("root fixture");
    let control = LocalTempCleanupControl::new(None);
    let filesystem = LocalFileSystems::rooted_with_id_and_temp_cleanup(
        FileSystemId::new("cleanup-indeterminate-source").expect("filesystem id"),
        root.path(),
        LocalResourcePolicy::standard(),
        control.clone(),
    )
    .expect("rooted filesystem");
    let mut directory = filesystem
        .create_temp_directory(TempOptions::default())
        .expect("temporary directory");
    let native_directory = root.path().join(directory.path().as_str().trim_start_matches('/'));

    std::fs::remove_dir(&native_directory).expect("remove original temporary source");
    let error = directory
        .cleanup()
        .expect_err("missing source makes authority indeterminate");
    assert_eq!(FsErrorKind::Indeterminate, error.kind());
    assert_eq!(Some(FsEffectState::Unchanged), error.effect_state());
    let native_error = error
        .source()
        .and_then(|source| source.downcast_ref::<LocalFileError>())
        .expect("native cleanup error is retained");
    assert_eq!(LocalFileErrorKind::NotFound, native_error.kind());
    assert_eq!(TempResourceState::Indeterminate, directory.state());

    std::fs::create_dir(&native_directory).expect("install replacement directory");
    control.set_limits(Some(limits(8, 100)));
    let error = directory
        .cleanup()
        .expect_err("larger cleanup budget must not restore source authority");
    assert_eq!(FsErrorKind::InvalidState, error.kind());
    assert_eq!(None, error.effect_state());
    assert_eq!(TempResourceState::Indeterminate, directory.state());
    assert!(native_directory.is_dir(), "replacement source must remain untouched");
}

#[test]
fn test_partial_cleanup_failure_maps_effect_and_retains_recoverable_source() {
    let root = tempfile::tempdir().expect("root fixture");
    let control = LocalTempCleanupControl::new(Some(limits(8, 100)));
    let filesystem = LocalFileSystems::rooted_with_id_and_temp_cleanup(
        FileSystemId::new("cleanup-partial-failure").expect("filesystem id"),
        root.path(),
        LocalResourcePolicy::standard(),
        control,
    )
    .expect("rooted filesystem");
    let mut directory = filesystem
        .create_temp_directory(TempOptions::default())
        .expect("temporary directory");
    let native_directory = root.path().join(directory.path().as_str().trim_start_matches('/'));
    std::fs::write(native_directory.join("first"), b"first").expect("first entry");
    std::fs::write(native_directory.join("second"), b"second").expect("second entry");
    let fault = install_test_fault("temp-directory-remove-second").expect("cleanup fault installs");

    let error = directory.cleanup().expect_err("injected second removal must fail");
    drop(fault);

    assert_eq!(FsErrorKind::Io, error.kind());
    assert_eq!(Some(FsEffectState::PartiallyApplied), error.effect_state());
    let native_error = error
        .source()
        .and_then(|source| source.downcast_ref::<LocalFileError>())
        .expect("native partial cleanup error is retained");
    assert_eq!(LocalFileErrorKind::PublicationIncomplete, native_error.kind());
    assert_eq!(LocalFileOperation::Cleanup, native_error.operation());
    assert_eq!(TempResourceState::CleanupRequired, directory.state());
    directory
        .cleanup()
        .expect("owned source can recover after transient failure");
    assert_eq!(TempResourceState::Cleaned, directory.state());
}

#[test]
fn test_cleanup_control_is_shared_by_sessions_and_isolated_between_filesystems() {
    let first_root = tempfile::tempdir().expect("first root fixture");
    let first_control = LocalTempCleanupControl::new(None);
    let first_filesystem = LocalFileSystems::rooted_with_id_and_temp_cleanup(
        FileSystemId::new("cleanup-shared-control").expect("filesystem id"),
        first_root.path(),
        LocalResourcePolicy::standard(),
        first_control.clone(),
    )
    .expect("first rooted filesystem");
    let mut first = first_filesystem
        .create_temp_directory(TempOptions::default())
        .expect("first temporary directory");
    let mut second = first_filesystem
        .create_temp_directory(TempOptions::default())
        .expect("second temporary directory");
    let first_path = first_root.path().join(first.path().as_str().trim_start_matches('/'));
    let second_path = first_root.path().join(second.path().as_str().trim_start_matches('/'));
    std::fs::write(first_path.join("entry"), b"data").expect("first entry");
    std::fs::write(second_path.join("entry"), b"data").expect("second entry");

    let second_root = tempfile::tempdir().expect("second root fixture");
    let second_control = LocalTempCleanupControl::new(None);
    let second_filesystem = LocalFileSystems::rooted_with_id_and_temp_cleanup(
        FileSystemId::new("cleanup-isolated-control").expect("filesystem id"),
        second_root.path(),
        LocalResourcePolicy::standard(),
        second_control.clone(),
    )
    .expect("second rooted filesystem");
    let mut isolated = second_filesystem
        .create_temp_directory(TempOptions::default())
        .expect("isolated temporary directory");
    let isolated_path = second_root
        .path()
        .join(isolated.path().as_str().trim_start_matches('/'));
    std::fs::write(isolated_path.join("entry"), b"data").expect("isolated entry");

    first_control.set_limits(Some(limits(8, 0)));
    assert_eq!(FsErrorKind::ResourceLimitExceeded, first.cleanup().unwrap_err().kind());
    assert_eq!(FsErrorKind::ResourceLimitExceeded, second.cleanup().unwrap_err().kind());
    isolated.cleanup().expect("independent control remains unbounded");

    first_control.set_limits(Some(limits(8, 100)));
    first.cleanup().expect("shared cleanup can recover");
    second.cleanup().expect("shared cleanup can recover");
}

#[test]
fn test_drop_uses_the_latest_cleanup_budget() {
    let root = tempfile::tempdir().expect("root fixture");
    let control = LocalTempCleanupControl::new(None);
    let filesystem = LocalFileSystems::rooted_with_id_and_temp_cleanup(
        FileSystemId::new("cleanup-drop-snapshot").expect("filesystem id"),
        root.path(),
        LocalResourcePolicy::standard(),
        control.clone(),
    )
    .expect("rooted filesystem");
    let directory = filesystem
        .create_temp_directory(TempOptions::default())
        .expect("temporary directory");
    let native_directory = root.path().join(directory.path().as_str().trim_start_matches('/'));
    std::fs::write(native_directory.join("entry"), b"data").expect("temporary entry");
    control.set_limits(Some(limits(8, 0)));

    drop(directory);

    assert!(native_directory.join("entry").exists());
}

#[cfg(feature = "registry")]
#[test]
fn test_rooted_provider_cleanup_control_reaches_resolved_filesystems() {
    use qubit_fs::path::ConnectionUri;
    use qubit_fs_local::LocalFileSystemProvider;
    use qubit_fs_registry::FileSystemConfig;
    use qubit_fs_registry::FileSystemRegistry;

    let root = tempfile::tempdir().expect("root fixture");
    let provider = LocalFileSystemProvider::rooted(
        FileSystemId::new("cleanup-registry-provider").expect("filesystem id"),
        root.path(),
        LocalResourcePolicy::standard(),
    )
    .expect("rooted provider");
    let shared = provider.temp_cleanup_control();
    let registry = FileSystemRegistry::default();
    registry.register(provider.clone()).expect("register provider clone");
    let resolution = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///entry").expect("URI"),
        ))
        .expect("resolve rooted provider");
    let mut directory = resolution
        .file_system()
        .create_temp_directory(TempOptions::default())
        .expect("temporary directory");
    let native_directory = root.path().join(directory.path().as_str().trim_start_matches('/'));
    std::fs::write(native_directory.join("entry"), b"data").expect("temporary entry");

    provider.temp_cleanup_control().set_limits(Some(limits(8, 0)));
    assert_eq!(
        FsErrorKind::ResourceLimitExceeded,
        directory.cleanup().unwrap_err().kind()
    );
    shared.set_limits(Some(limits(8, 100)));
    directory
        .cleanup()
        .expect("provider clones share cleanup configuration");
}

#[cfg(feature = "registry")]
#[test]
fn test_host_provider_cleanup_control_reaches_later_resolutions() {
    use qubit_fs::path::ConnectionUri;
    use qubit_fs_local::LocalFileSystemProvider;
    use qubit_fs_registry::FileSystemConfig;
    use qubit_fs_registry::FileSystemRegistry;

    let parent = tempfile::tempdir().expect("host parent fixture");
    let logical_parent = host_path_to_logical(parent.path()).expect("logical host parent");
    let provider = LocalFileSystemProvider::host(LocalResourcePolicy::standard());
    let shared = provider.temp_cleanup_control();
    let registry = FileSystemRegistry::default();
    registry.register(provider.clone()).expect("register provider clone");
    let resolution = registry
        .resolve_config(&FileSystemConfig::new(
            ConnectionUri::parse("file:///tmp").expect("URI"),
        ))
        .expect("resolve host provider");
    let mut directory = resolution
        .file_system()
        .create_temp_directory(TempOptions::default().with_parent(Some(logical_parent)))
        .expect("temporary directory");
    let sandbox = std::fs::read_dir(parent.path())
        .expect("read host parent")
        .next()
        .expect("temporary sandbox")
        .expect("sandbox entry")
        .path();
    let native_directory = std::fs::read_dir(sandbox)
        .expect("read temporary sandbox")
        .next()
        .expect("one generated temporary directory")
        .expect("temporary directory entry")
        .path();
    std::fs::write(native_directory.join("entry"), b"data").expect("temporary entry");

    shared.set_limits(Some(limits(8, 0)));
    assert_eq!(
        FsErrorKind::ResourceLimitExceeded,
        directory.cleanup().unwrap_err().kind()
    );
    provider.temp_cleanup_control().set_limits(Some(limits(8, 100)));
    directory
        .cleanup()
        .expect("host provider clones share cleanup configuration");
}

#[cfg(feature = "registry")]
#[test]
fn test_registry_host_constructor_remains_const_usable() {
    let _provider = &CONST_HOST_PROVIDER;
}

fn limits(max_depth: usize, max_entries: usize) -> LocalTempCleanupResourceLimits {
    LocalTempCleanupResourceLimits::new(max_depth, max_entries, 1 << 20, Duration::from_secs(30))
}
