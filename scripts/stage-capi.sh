#!/usr/bin/env bash
# Package one C ABI release archive for one target:
#   dist-capi/skopli-capi-<version>-<target>.tar.gz   (.zip on Windows targets)
# containing skopli.h plus the shared and static libraries from
# target/<target>/release. The version is the skopli-capi crate version.
#
# Usage: scripts/stage-capi.sh <target-triple>   (after `cargo build -p skopli-capi --release --target <target-triple>`)
set -euo pipefail

target="${1:?usage: stage-capi.sh <target-triple>}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "skopli-capi") | .version')"
stem="skopli-capi-$version-$target"
stage="dist-capi/$stem"
rm -rf "$stage" "$stage".tar.gz "$stage".zip
mkdir -p "$stage"

cp crates/skopli-capi/include/skopli.h "$stage/"
found=0
for name in libskopli.so libskopli.dylib libskopli.a skopli.dll skopli.dll.lib skopli.lib; do
  f="target/$target/release/$name"
  if [ -f "$f" ]; then
    cp "$f" "$stage/"
    found=1
  fi
done
if [ "$found" = 0 ]; then
  echo "no skopli library under target/$target/release; build it first" >&2
  exit 1
fi

cd dist-capi
case "$target" in
  *-windows-*) archive="$stem.zip"; 7z a -tzip -bso0 "$archive" "$stem" ;;
  *) archive="$stem.tar.gz"; tar -czf "$archive" "$stem" ;;
esac
rm -r "$stem"
echo "dist-capi/$archive"
