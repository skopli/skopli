#!/usr/bin/env bash
# Regenerate the committed C ABI header and assert it is a zero-diff (the header
# is CI-diffed append-only per reconciliation Decision 1: it can only grow, and
# an out-of-date committed header is a build failure).
#
# Usage: scripts/check-capi-header.sh
# Exit 0 when the committed header matches a fresh cbindgen run; non-zero (with
# the diff printed) otherwise.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
header="$repo_root/crates/skopli-capi/include/skopli.h"

if [ ! -f "$header" ]; then
  echo "committed header missing: $header" >&2
  exit 1
fi

before="$(cat "$header")"

# Regenerate in place via the build script (gated on the env var).
SKOPLI_GEN_HEADER=1 cargo build -p skopli-capi >/dev/null 2>&1

after="$(cat "$header")"

if [ "$before" != "$after" ]; then
  echo "C ABI header is out of date. Regenerate with:" >&2
  echo "  SKOPLI_GEN_HEADER=1 cargo build -p skopli-capi" >&2
  echo "--- diff ---" >&2
  diff <(printf '%s' "$before") <(printf '%s' "$after") >&2 || true
  exit 1
fi

echo "C ABI header is up to date (zero-diff)."
