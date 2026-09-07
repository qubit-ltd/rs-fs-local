# Qubit FS Local Adapter Design

> Applies to `qubit-fs-local` 0.4. This document describes the public boundary
> and mapping contract used by the implementation and regression tests.
>
> [中文设计文档](local_file_system_adapter_design.zh_CN.md) ·
> [User guide](user_guide.md)

## 1. Positioning

`qubit-fs-local` is a thin adapter between `qubit-fs` and
`qubit-local-files`:

```text
qubit-fs facade and SPI contracts
              ▲
              │ implements FileSystemSpi
      qubit-fs-local
              │ delegates
              ▼
qubit-local-files native semantics and platform code
```

The adapter translates types, paths, errors, outcomes, and sessions. It does
not implement a second local filesystem algorithm.

## 2. Goals and non-goals

The adapter provides a synchronous local `FileSystem`, implements
`FileSystemSpi`, delegates native work to `qubit-local-files`, exposes accurate
properties and provider-neutral effect states, and optionally registers a
validated `file:` provider with `qubit-fs-registry`. It does not implement
copy, walking, temporary resources, publication, durability, containment, or
platform-specific filesystem algorithms; it does not wrap blocking I/O as an
async runtime API or expose operation SPI as the ordinary application API.

## 3. Construction and resource policy

`LocalFileSystems` is a namespace-only factory. Every constructor requires a
`LocalResourcePolicy`:

```rust
pub fn host(policy: LocalResourcePolicy) -> FsResult<FileSystem>;
pub fn rooted(root: &Path, policy: LocalResourcePolicy) -> FsResult<FileSystem>;
pub fn rooted_with_id(
    id: FileSystemId,
    root: &Path,
    policy: LocalResourcePolicy,
) -> FsResult<FileSystem>;
```

`LocalResourcePolicy::bounded(list, copy, delete)` sets independent per-request
ceilings for listing, copying, and ordinary recursive deletion.
`unbounded()` is an explicit opt-in. Listing budgets count native walker work
before adapter prefix filtering; request limits count entries returned after
filtering. Delete budgets count the requested root at depth zero and pending
encoded native path bytes. Deadlines are cooperative. Temporary cleanup, Drop,
and native publication cleanup have separate lifecycle semantics and do not
inherit ordinary delete ceilings; these settings are not aggregate concurrent
quotas.

## 4. Properties and path mapping

The SPI returns stable, I/O-free `FileSystemProperties`: configured identity,
provider identity, hierarchical path semantics, authority description, native
capabilities and conservative limits. Native component limits cannot be
converted into logical UTF-8 text limits because percent expansion and
non-UTF-8 names change the representation, so unknown values remain
`FileSystemLimit::Unknown`.

The private path mapper delegates codec and platform rules to
`qubit-local-files`. `host_path_to_logical` is the public entry point for an
absolute native host path and preserves percent escapes and non-UTF-8 names.
Logical paths are absolute and hierarchical; rooted `/` denotes the retained
native root. All paths in a multi-path request are converted before native I/O
so a later conversion failure cannot leave an earlier side effect.

## 5. Operation, outcome, and session mapping

SPI methods perform only request conversion, one native call, and outcome
mapping. Copy first classifies all options without side effects. An
unrepresentable copy returns `RequirementNotMet` with unchanged state and zero
partial statistics; once native copy starts, every native failure is terminal
and is never changed into facade fallback. Native copy and rename states,
partial statistics, and effect states are preserved.

Readers and writers retain the requested logical path and filesystem identity.
The writer adapter preserves publication method, atomicity, durability, and
`NotPublished`, `RetryableNotPublished`, `Published`, and `Indeterminate`
states; it does not perform its own rename, fsync, or staging cleanup. Lazy
native directory streams are adapted entry by entry rather than preloaded into
a `Vec`.

Temporary file and directory sessions retain the originating `FileSystem`.
Rooted temporary operations continue using the original native root authority,
including persist, cleanup, and child operations. Persist state maps one to one
to provider-neutral state. Explicit cleanup reports errors; Drop is best
effort. An unknown native source/target state maps to `Indeterminate`.

## 6. Errors and registry integration

Error mapping is centralized in private functions in `spi/error_mapper.rs`.
Typed I/O sources take precedence over fallback native kinds; invalid paths and
options remain distinct from `NotDirectory`, `IsDirectory`, and conflicts.
Provider and logical/target path context comes from the request and descriptor.
`cause_kind()` selects the base error kind while `effect_state()` adds only
proven side effects. Impossible adapter states use
`ProviderContractViolation`.

With the `registry` feature, `LocalFileSystemProvider` exposes the default
`local-file` descriptor and `file` alias, accepts absolute local `file:` URIs,
and returns a concrete filesystem resolution and canonical URI. Remote
authorities, query/options/credentials, relative paths, and non-`file` schemes
are rejected. Unsupported schemes may participate in `OnAbsence` fallback;
recognized-but-invalid `file:` configurations and rooted initialization errors
remain terminal. A canonical URI does not encode rooted authority or identity,
so replay must also persist provider selection/descriptor and `FileSystemId`.

## 7. Async and platform boundaries

Version 0.4 implements only synchronous `FileSystemSpi`. It does not create an
async facade, boxed futures, or a runtime around blocking I/O. Platform
business rules for copy, walking, symlinks, containment, publication, and
durability belong in `qubit-local-files`; this adapter may contain only
representation-level path conversion.

## 8. Module organization

```text
src/
├── constants.rs
├── local_file_systems.rs
├── local_*_resource_limits.rs
├── spi/ (SPI sessions, option/outcome/error mappers)
├── path/local_path_mapper.rs
└── registry/ (provider and URI path mapping)
```

Host and rooted construction share `LocalFileSystemSpi`; the native scope
distinguishes their authorities. Shared conversion belongs in focused private
mappers and sessions, not in a platform-heavy monolithic adapter.

## 9. Verification strategy

Tests are layered: mapper unit tests prove one-to-one request/error/outcome
conversion; adapter integration tests cover host/rooted construction, path
conversion-before-I/O, copy/rename typed states, writer and temporary terminal
states, and registry resolution; and the public facade runs the applicable
`qubit-fs-testkit::FileSystemContractSuite`. Native platform algorithms are
tested by `qubit-local-files`; this crate verifies that translation does not
lose their semantics. The `fuzz/registry_uri_resolution.rs` target exercises
URI resolution without filesystem mutation.
