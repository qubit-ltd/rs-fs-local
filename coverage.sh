#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
export PROPTEST_RNG_SEED="${PROPTEST_RNG_SEED:-1}"
"$project_root/.infra/tools/prepare-local-path-dependencies.sh"
exec "$project_root/.infra/tools/infra-tool.sh" rs-infra-coverage --project "$project_root" collect "$@"
