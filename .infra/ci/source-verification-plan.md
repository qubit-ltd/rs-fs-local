# Ordinary source verification plan

The adapter routes the shared `verify` entry through the supported source
suites. The `package` archive suite is reserved for an explicitly authorized
release check.

## Local full plan

`./.infra/bin/ci-check.sh` runs these source suites first:

```text
rs-infra-verify run --suite lock
rs-infra-verify run --suite build
rs-infra-verify run --suite test
rs-infra-verify run --suite doc
```

It then runs `rs-infra-ci check` with the tasks in `ci.toml`: style, Clippy,
coverage-configured Clippy, strict docs, README, release build, feature matrix,
fuzz, project hook, threshold-enforced coverage, and audit.

## Verify-only plan

`./.infra/bin/ci-check.sh --only=verify` runs only the same four source suites:
lock, build, test, and doc. It does not invoke package verification.

## Hosted workflow command comparison

The reusable workflow calls `./.infra/bin/ci-check.sh --only=verify`; the
adapter wrapper routes that command to the four source suites above. The
reusable workflow's separate strict rustdoc, README, style, feature-matrix,
fuzz, threshold-enforced coverage, and platform jobs remain enabled. The adapter's Windows job
continues to prepare local path dependencies and run its registry-feature test
matrix.

No ordinary workflow command invokes `rs-infra-verify run --suite package` or
`cargo package`.

The documentation profile harness logs each resolved package identity, source,
version, feature set, and candidate repository SHA. Compare those lines from the
hosted prepared-source layout with a full workspace run during unified
verification.
