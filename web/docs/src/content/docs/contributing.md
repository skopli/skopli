---
title: Contributing
description: How to build and test Skopli, add a harness or a pricing source, and what a pull request needs.
---

Skopli is one Rust core with facades in nine languages.
Most changes land in the core and show up in every language at once.
The facades only change when the wire format or the public API does.

## Where things live

| Path                                                           | What it is                                                                                                                                  |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| [`crates/skopli-core`](https://github.com/skopli/skopli/tree/main/crates/skopli-core)                                           | Readers for every harness ([`crates/skopli-core/src/readers/`](https://github.com/skopli/skopli/tree/main/crates/skopli-core/src/readers/)), the rollup engine, and the pricing layer ([`crates/skopli-core/src/pricing/`](https://github.com/skopli/skopli/tree/main/crates/skopli-core/src/pricing/)) |
| [`crates/skopli-wire`](https://github.com/skopli/skopli/tree/main/crates/skopli-wire)                                           | The JSON wire format shared by every facade                                                                                                 |
| [`crates/skopli-capi`](https://github.com/skopli/skopli/tree/main/crates/skopli-capi)                                           | The C ABI (`libskopli`) with its committed, zero-diff-checked header `include/skopli.h`                                                     |
| [`crates/skopli-node`](https://github.com/skopli/skopli/tree/main/crates/skopli-node), [`crates/skopli-py`](https://github.com/skopli/skopli/tree/main/crates/skopli-py), [`crates/skopli-ruby`](https://github.com/skopli/skopli/tree/main/crates/skopli-ruby) | Native extensions for TypeScript (napi-rs), Python (PyO3), and Ruby (Magnus)                                                                |
| [`src/`](https://github.com/skopli/skopli/tree/main/src/), [`test/`](https://github.com/skopli/skopli/tree/main/test/)                                                | The TypeScript facade and its tests                                                                                                         |
| [`sdks/go`](https://github.com/skopli/skopli/tree/main/sdks/go), [`sdks/java`](https://github.com/skopli/skopli/tree/main/sdks/java), [`sdks/csharp`](https://github.com/skopli/skopli/tree/main/sdks/csharp), [`sdks/swift`](https://github.com/skopli/skopli/tree/main/sdks/swift)            | Facades that load `libskopli` through cgo, FFM, P/Invoke, and a SwiftPM system-library target                                               |
| [`golden/`](https://github.com/skopli/skopli/tree/main/golden/)                                                      | The conformance corpus: real-shaped inputs and exact expected outputs for every harness, rollup, and pricing case                           |
| [`web/`](https://github.com/skopli/skopli/tree/main/web/)                                                         | The marketing site (`site`), the docs site (`docs`), and the design system they share (`ui`)                                                |

`sdks/*`, [`crates/skopli-py`](https://github.com/skopli/skopli/tree/main/crates/skopli-py), and [`crates/skopli-ruby`](https://github.com/skopli/skopli/tree/main/crates/skopli-ruby) each have a README with their own build and test steps.

## Build

You need Rust, Node 24, and pnpm.
[`rust-toolchain.toml`](https://github.com/skopli/skopli/blob/main/rust-toolchain.toml) pins the Rust toolchain, and the workspace `rust-version` is the 1.88 floor.
Use pnpm for every Node command.
The repository does not use npm or yarn.

`pnpm run build:native` builds the Node addon, and `cargo build -p skopli-capi --release` builds `libskopli` for the Go, Java, C#, and Swift facades.
On Windows the Go facade links the GNU target instead, as [`sdks/go/README.md`](https://github.com/skopli/skopli/blob/main/sdks/go/README.md) describes.

```sh
pnpm install
pnpm run build:native
cargo build -p skopli-capi --release
```

The default `libskopli` opens no socket.
The `net` cargo feature adds built-in catalog fetching for C consumers.

## Test

The local gate has two parts.
The Rust format, lint, and test commands come first.
`pnpm run check` runs oxlint, the prose rules in [`scripts/check-prose.mjs`](https://github.com/skopli/skopli/blob/main/scripts/check-prose.mjs) (no em dash in any tracked text and one sentence per line in Markdown paragraphs; captured harness input under [`golden/`](https://github.com/skopli/skopli/tree/main/golden/) and third-party notices are exempt), oxfmt, tsc over the facade, the shared UI package and both sites, vitest, and the generated-file check.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --exclude skopli --all-targets -- -D warnings
cargo test --workspace --exclude skopli
pnpm run check
```

CI runs the same commands across a Rust matrix (1.88, 1.89, stable) and a Node addon matrix, plus both site builds, the Python, Go, and .NET facades on Ubuntu, macOS, and Windows, Ruby and Java on Ubuntu and macOS, and Swift on macOS.

`cargo test` replays every case under [`golden/`](https://github.com/skopli/skopli/tree/main/golden/) through the core.
A change to an `expected-*.json` file is a documented behavior change.
Say why in the commit message.
[`golden/README.md`](https://github.com/skopli/skopli/blob/main/golden/README.md) describes the corpus layout and how to add a case.

Five files carry generated content, each block from a source elsewhere in the repository, and `pnpm run sync --check` (part of `pnpm run check`) fails when a copy drifts:

- the install table in [`README.md`](https://github.com/skopli/skopli/blob/main/README.md), from [`web/ui/src/quickstart.ts`](https://github.com/skopli/skopli/blob/main/web/ui/src/quickstart.ts)
- the supported-harness table in [`README.md`](https://github.com/skopli/skopli/blob/main/README.md), from [`web/ui/src/harnesses.ts`](https://github.com/skopli/skopli/blob/main/web/ui/src/harnesses.ts) (names, ids and which rows carry a logo from [`assets/logos/`](https://github.com/skopli/skopli/tree/main/assets/logos/))
- the supported-harness list in [`llms.txt`](https://github.com/skopli/skopli/blob/main/llms.txt), from [`golden/registry/ids.json`](https://github.com/skopli/skopli/blob/main/golden/registry/ids.json)
- the docs copy of this page, from [`CONTRIBUTING.md`](https://github.com/skopli/skopli/blob/main/CONTRIBUTING.md)
- the C header [`crates/skopli-capi/include/skopli.h`](https://github.com/skopli/skopli/blob/main/crates/skopli-capi/include/skopli.h), from the crate's `extern "C"` surface through cbindgen
- the C# P/Invoke layer [`sdks/csharp/Skopli/Native/NativeMethods.g.cs`](https://github.com/skopli/skopli/blob/main/sdks/csharp/Skopli/Native/NativeMethods.g.cs), from the same surface through csbindgen

Edit the source and run `pnpm run sync`.
On the docs copy, every backticked path that exists in the repository becomes a link to the file on GitHub.
The links change the table widths, so that copy is excluded from oxfmt.

## Add a harness

1. Write the reader in `crates/skopli-core/src/readers/<id>.rs`.
   Declare it in `readers/mod.rs` and append it to `registered_readers()` in `readers/registry.rs`.
2. Append the id to [`golden/registry/ids.json`](https://github.com/skopli/skopli/blob/main/golden/registry/ids.json).
   The conformance test asserts the file matches `registered_readers()` in order.
3. Add at least one case under `golden/<id>/` with `input/` files captured from the real application and sanitized, an `input-manifest.json`, and the expected outputs.
   Spot-check a headline figure against the application's own UI before committing.
4. Add the id to the harness enums in the facades: [`src/types.ts`](https://github.com/skopli/skopli/blob/main/src/types.ts) and [`src/detect.ts`](https://github.com/skopli/skopli/blob/main/src/detect.ts), [`crates/skopli-py/python/skopli/_enums.py`](https://github.com/skopli/skopli/blob/main/crates/skopli-py/python/skopli/_enums.py), [`sdks/go/skopli/types.go`](https://github.com/skopli/skopli/blob/main/sdks/go/skopli/types.go), [`sdks/java/src/main/java/com/skopli/Harness.java`](https://github.com/skopli/skopli/blob/main/sdks/java/src/main/java/com/skopli/Harness.java), [`sdks/csharp/Skopli/Models.cs`](https://github.com/skopli/skopli/blob/main/sdks/csharp/Skopli/Models.cs), [`sdks/swift/Sources/Skopli/Models.swift`](https://github.com/skopli/skopli/blob/main/sdks/swift/Sources/Skopli/Models.swift).
5. Add the display name (and an icon, if an official mark exists under a permissive license) to [`web/ui/src/harnesses.ts`](https://github.com/skopli/skopli/blob/main/web/ui/src/harnesses.ts), and a row to the harness table in [`README.md`](https://github.com/skopli/skopli/blob/main/README.md).
   New icons need an entry in [`THIRD-PARTY-NOTICES.md`](https://github.com/skopli/skopli/blob/main/THIRD-PARTY-NOTICES.md).

Readers never touch the network.
A field the harness does not record is `0`, and a timestamp that cannot be parsed is a diagnostic, not an epoch-0 event.
A reader estimates tokens only when the harness stores none, and [`README.md`](https://github.com/skopli/skopli/blob/main/README.md), [`llms.txt`](https://github.com/skopli/skopli/blob/main/llms.txt), and the docs then state the estimate for that harness (Kiro today).

## Add a pricing source

Built-in catalogs live in [`crates/skopli-core/src/pricing/sources.rs`](https://github.com/skopli/skopli/blob/main/crates/skopli-core/src/pricing/sources.rs), one `SourceSpec` per catalog with its URL and cache name.
`pricing/parse.rs` parses each format into a `PriceMap`, which `sources.rs` wraps as a `PricingCatalog`.

1. Add a `SourceUrls` field and a `builtin_specs` slot in `sources.rs`.
2. Write the parser in `parse.rs`.
3. Add the source to every facade's table.
   Each facade fetches in its own language, so each carries a copy (`rg models.dev/api.json` lists them), and [`test/pricing.test.ts`](https://github.com/skopli/skopli/blob/main/test/pricing.test.ts) pins the TypeScript copy to [`golden/pricing/lifecycle/constants.json`](https://github.com/skopli/skopli/blob/main/golden/pricing/lifecycle/constants.json).
4. Add a fixture under [`golden/pricing/`](https://github.com/skopli/skopli/tree/main/golden/pricing/).
5. Cover it in [`crates/skopli-core/tests/pricing_conformance.rs`](https://github.com/skopli/skopli/blob/main/crates/skopli-core/tests/pricing_conformance.rs).
   Unknown models must keep returning `priced: false` with the attempted keys.
   A source never invents a price.

## Pull requests

- Branch from `main`.
  Every change lands through a pull request.
- The PR title is a Conventional Commit (`feat(web): ...`, `fix(core): ...`) and the PR carries one independently releasable change.
- CI must be green, including the generated-file check above.
- Docs and marketing copy state only what the code does: no benchmarks, adoption numbers, or testimonials.

## Releases

[`.github/workflows/release.yml`](https://github.com/skopli/skopli/blob/main/.github/workflows/release.yml) builds every facade on a tag and attaches the artifacts to a draft GitHub release.
Before tagging `vX.Y.Z`, set `X.Y.Z` in the workspace [`Cargo.toml`](https://github.com/skopli/skopli/blob/main/Cargo.toml) (`workspace.package.version` and the `skopli-core` and `skopli-wire` entries under `workspace.dependencies`), in [`package.json`](https://github.com/skopli/skopli/blob/main/package.json), in [`crates/skopli-py/pyproject.toml`](https://github.com/skopli/skopli/blob/main/crates/skopli-py/pyproject.toml), in [`crates/skopli-ruby/lib/skopli/version.rb`](https://github.com/skopli/skopli/blob/main/crates/skopli-ruby/lib/skopli/version.rb) (`Skopli::VERSION`), in [`sdks/java/build.gradle.kts`](https://github.com/skopli/skopli/blob/main/sdks/java/build.gradle.kts), and in [`sdks/csharp/Skopli/Skopli.csproj`](https://github.com/skopli/skopli/blob/main/sdks/csharp/Skopli/Skopli.csproj).
The crates.io publish job refuses a tag that does not match the crate version; the other manifests are not checked.
`scripts/stage-capi.sh <target>` builds the C ABI archive the workflow attaches; run it after `cargo build -p skopli-capi --release --target <target>` to inspect one.
The registry publish jobs are gated behind the `PUBLISH_ENABLED` repository variable.
[`PUBLISHING-SETUP.md`](https://github.com/skopli/skopli/blob/main/PUBLISHING-SETUP.md) lists the trusted-publisher setup each registry needs.
When the first release ships, set `published = true` in [`web/ui/src/quickstart.ts`](https://github.com/skopli/skopli/blob/main/web/ui/src/quickstart.ts) and run `pnpm run sync`.
The README switches from repository installs to registry installs, and the sites and their generated Markdown follow on their next build.
