#!/usr/bin/env bash
# Regenerate the jextract FFM bindings for the skopli C ABI.
#
# The bindings are COMMITTED under src/main/java/com/skopli/ffi/ (mirroring
# the committed C header); this script reproduces them and
# check-bindings.sh asserts a zero diff in CI, so the committed bindings can
# never drift from crates/skopli-capi/include/skopli.h.
#
# jextract runs on its own bundled runtime; point JEXTRACT at its launcher
# (e.g. JEXTRACT=/path/to/jextract-22/bin/jextract).
# Only the 15 ag_* functions + AgBuf/AgStatus are included (the opaque AgPricing
# handle crosses as a raw pointer, so its struct is intentionally excluded - it
# has no fields anyway).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sdk_root="$(cd "$here/.." && pwd)"
repo_root="$(cd "$sdk_root/../.." && pwd)"

JEXTRACT="${JEXTRACT:-jextract}"
header="$repo_root/crates/skopli-capi/include/skopli.h"
out_dir="$sdk_root/src/main/java"
pkg_dir="$out_dir/com/skopli/ffi"

rm -f "$pkg_dir"/*.java

"$JEXTRACT" \
  --target-package com.skopli.ffi \
  --header-class-name Skopli \
  --output "$out_dir" \
  --include-function ag_abi_version \
  --include-function ag_schema_version \
  --include-function ag_version \
  --include-function ag_buf_free \
  --include-function ag_string_free \
  --include-function ag_last_error_message \
  --include-function ag_detect_harnesses \
  --include-function ag_read_usage \
  --include-function ag_rollup \
  --include-function ag_cost_usd \
  --include-function ag_default_cache_dir \
  --include-function ag_pricing_new \
  --include-function ag_pricing_free \
  --include-function ag_pricing_price_events \
  --include-function ag_pricing_catalog_info \
  --include-typedef AgBuf \
  --include-typedef AgStatus \
  "$header"

echo "jextract bindings regenerated into $pkg_dir"
