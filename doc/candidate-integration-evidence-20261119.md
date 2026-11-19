# Candidate local-files 0.6 integration evidence

This records the T3 candidate-mode verification in the assigned downstream worktrees.

## Revisions and lockfiles

| Repository | Base revision used for verification | Candidate `Cargo.lock` SHA-256 |
| --- | --- | --- |
| `rs-fs-local` | `1067ff1ffe75376f2349c2a484ccf2423e29d20e` | `1d689273a043dd326d0c6d2e2373797906fd771b19c1bbe08e60c3f19519407b` |
| `rs-mime` | `147903915c1fa0f57f4d6eb458ce92d207dd3dac` | `69629bd7037a7c997757bb808dcd83f5dcc59371e5dff2e594e2688d02f02275` |

Before candidate integration, both base lockfiles resolved `qubit-local-files 0.5.3` from the crates.io registry. The candidate lockfiles resolve the 0.6.0 path package below.

## Resolved package identity

`cargo metadata --locked --all-features --format-version 1` was run in both repository roots. MIME metadata showed both its direct `qubit-local-files` edge and the `qubit-fs-local` transitive edge point to the same package:

- Package ID: `path+file:///home/starfish/.local/share/agent-worktrees/8de8f36f-c31f-45fd-9b69-09c333e590b4-round-3/layout/rust-common/rs-local-files#qubit-local-files@0.6.0`
- Version: `0.6.0`
- Source: `null` (path package)
- Manifest: `/home/starfish/.local/share/agent-worktrees/8de8f36f-c31f-45fd-9b69-09c333e590b4-round-3/layout/rust-common/rs-local-files/Cargo.toml`

The provider documentation test checks the outer provider dependency edge and each enabled nested documentation consumer edge against the candidate manifest path, version, and source. Its helper tests accept the candidate identity and reject the old registry 0.5.3 package.

## Commands and results

From each assigned repository root:

```text
cargo metadata --locked --all-features --format-version 1
```

Both commands succeeded.

From `rs-fs-local`:

```text
QUBIT_LOCAL_FILES_CANDIDATE_MANIFEST=/home/starfish/.local/share/agent-worktrees/8de8f36f-c31f-45fd-9b69-09c333e590b4-round-3/layout/rust-common/rs-local-files/Cargo.toml cargo test --locked --all-features --test local_error_mapping_tests --test local_temp_resource_spi_tests --test documentation_examples_tests
```

Passed: documentation examples and identity checks (17), local error mapping (5), and local temp resource SPI (34).

From `rs-mime`:

```text
QUBIT_LOCAL_FILES_CANDIDATE_MANIFEST=/home/starfish/.local/share/agent-worktrees/8de8f36f-c31f-45fd-9b69-09c333e590b4-round-3/layout/rust-common/rs-local-files/Cargo.toml cargo test --locked --all-features --lib managed_staging_tests
```

Passed: all 18 managed staging tests.
