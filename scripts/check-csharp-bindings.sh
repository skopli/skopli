#!/usr/bin/env bash
# Regenerate the committed C# P/Invoke bindings and assert it is a zero-diff.
# The csbindgen-generated NativeMethods.g.cs is committed and CI checks that a
# fresh regeneration produces no change - the generated P/Invoke surface must
# always match the Rust `extern "C"` symbols (mirrors scripts/check-capi-header.sh).
#
# Usage: scripts/check-csharp-bindings.sh
# Exit 0 when the committed bindings match a fresh csbindgen run; non-zero (with
# the diff printed) otherwise.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bindings="$repo_root/sdks/csharp/Skopli/Native/NativeMethods.g.cs"

if [ ! -f "$bindings" ]; then
  echo "committed C# bindings missing: $bindings" >&2
  exit 1
fi

before="$(cat "$bindings")"

# Regenerate in place via the build script (gated on the env var).
SKOPLI_GEN_CSHARP=1 cargo build -p skopli-capi >/dev/null 2>&1

after="$(cat "$bindings")"

if [ "$before" != "$after" ]; then
  echo "C# P/Invoke bindings are out of date. Regenerate with:" >&2
  echo "  SKOPLI_GEN_CSHARP=1 cargo build -p skopli-capi" >&2
  echo "--- diff ---" >&2
  diff <(printf '%s' "$before") <(printf '%s' "$after") >&2 || true
  exit 1
fi

echo "C# P/Invoke bindings are up to date (zero-diff)."
