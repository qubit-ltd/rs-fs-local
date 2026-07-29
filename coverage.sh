#!/bin/bash
set -euo pipefail

PROJECT_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
exec env \
    COVERAGE_ENFORCE_THRESHOLDS="${COVERAGE_ENFORCE_THRESHOLDS:-0}" \
    RS_CI_PROJECT_ROOT="$PROJECT_ROOT" \
    "$PROJECT_ROOT/.rs-ci/coverage.sh" "$@"
