#!/usr/bin/env bash
# Assert the committed jextract bindings match a fresh regeneration (zero diff).
# Mirrors scripts/check-capi-header.sh for the C header. Run in CI after any C
# ABI change to catch a stale committed binding.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sdk_root="$(cd "$here/.." && pwd)"
pkg_dir="$sdk_root/src/main/java/com/skopli/ffi"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cp -r "$pkg_dir"/*.java "$tmp/" 2>/dev/null || true

bash "$here/regen-bindings.sh" >/dev/null

if ! diff -r "$tmp" "$pkg_dir" >/dev/null 2>&1; then
  echo "ERROR: jextract bindings are stale. Run scripts/regen-bindings.sh and commit." >&2
  diff -r "$tmp" "$pkg_dir" >&2 || true
  exit 1
fi
echo "Java FFM bindings are up to date (zero-diff)."
