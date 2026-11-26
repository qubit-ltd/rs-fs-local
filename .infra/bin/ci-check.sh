#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)

# The shared verify task includes the package archive suite. Route ordinary
# verification through its lock/build/test/doc suites instead.
route=""
if [[ $# -eq 0 ]]; then
    route=source-full
elif [[ $# -eq 1 && "$1" == "plan" ]]; then
    route=source-full-plan
elif [[ $# -eq 1 && "$1" == "--only=verify" ]]; then
    route=source-verify
elif [[ $# -eq 2 && "$1" == "--only" && "$2" == "verify" ]]; then
    route=source-verify
elif [[ $# -eq 2 && "$1" == "--only=verify" && "$2" == "plan" ]]; then
    route=source-verify-plan
elif [[ $# -eq 3 && "$1" == "--only" && "$2" == "verify" && "$3" == "plan" ]]; then
    route=source-verify-plan
fi

if [[ -n "$route" ]]; then
    export RS_INFRA_CI_ROUTE="$route"
fi

worker_package="$project_root/worker/package.json"
if [[ -f "$worker_package" ]] && grep -Eq '"build:test-worker"[[:space:]]*:' "$worker_package"; then
    command -v node >/dev/null || { echo "error: Node.js is required to build the test worker" >&2; exit 1; }
    command -v corepack >/dev/null || { echo "error: Corepack is required to build the test worker" >&2; exit 1; }
    (
        cd "$project_root/worker"
        corepack pnpm install --frozen-lockfile --ignore-scripts
        corepack pnpm run build:test-worker
    )
fi

exec "$project_root/.infra/bootstrap.sh" .infra/bin/ci-check.sh "$@"
