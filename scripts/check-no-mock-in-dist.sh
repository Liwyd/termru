#!/usr/bin/env bash
# Fails when the dev-only mock backend (apps/desktop/src/lib/dev/mock-ipc.ts)
# or its fixtures reached a production build.
#
#   npm run build && scripts/check-no-mock-in-dist.sh [dist-dir]
#
# The bundle (everything but the source maps) must not contain the module
# name or any fixture string. The source maps must not list the module as a
# source (ipc-backend.ts legitimately mentions its path in a comment, so the
# maps are checked by their `sources`, not by text).
set -euo pipefail

DIST="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/apps/desktop/dist}"
[[ -d "$DIST" ]] || { echo "check-no-mock: no build output at $DIST (run: npm run build)" >&2; exit 1; }

patterns=(
  'mock-ipc' 'mock-session' 'mockCommands'
  'atlas-prod' 'relay-edge' 'staging-api' 'build-runner'
  'work-ed25519' 'build-ed25519'
  'database is locked (mock error)' 'a6b8d21'
)

status=0
for pattern in "${patterns[@]}"; do
  if grep -rIl --exclude='*.map' -F -- "$pattern" "$DIST"; then
    echo "check-no-mock: '$pattern' found in the bundle" >&2
    status=1
  fi
done

while IFS= read -r map; do
  if node -e '
    const map = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    process.exit((map.sources ?? []).some((source) => source.includes("/dev/mock-ipc")) ? 1 : 0);
  ' "$map"; then
    :
  else
    echo "check-no-mock: $map lists mock-ipc as a source" >&2
    status=1
  fi
done < <(find "$DIST" -name '*.map')

((status == 0)) && echo "check-no-mock: ok ($DIST)"
exit "$status"
