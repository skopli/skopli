# skopli (Swift SDK)

Idiomatic Swift bindings for skopli, riding the C ABI (`skopli-capi`)
through a hand-written SwiftPM wrapper over the generated C header (NOT UniFFI).
`Codable` models in/out, `throws` for errors (`SkopliError`), camelCase
throughout, `enum Harness: String`, sync functions plus `async` conveniences via
`withCheckedThrowingContinuation`. A `Pricing` `final class` frees the native
handle in `deinit`.

## Status: structure-complete, verified in CI

The Swift toolchain is not installed on the authoring host, so `swift build` /
`swift test` are **verified in CI on a macOS runner** (see below). The package is
delivered structure-complete: every C function signature was cross-checked
character-by-character against the generated header, and every JSON key mirrors
the sibling Go/Java/C# facades exactly.

## Layout

```
sdks/swift/
  Package.swift                       # systemLibrary + facade + test targets
  Sources/CSkopli/module.modulemap# imports the C header IN PLACE
  Sources/Skopli/                 # the pure-Swift idiomatic facade
    Models.swift  Errors.swift  Options.swift
    Skopli.swift  Pricing.swift  Ffi.swift  JSONAny.swift
  Tests/SkopliTests/SmokeTests.swift
```

The module map references the committed header directly from the capi crate:

```
header "../../../../crates/skopli-capi/include/skopli.h"
```

so the Swift build always sees the current cbindgen output. The header IS the
binding - there is no generated intermediate to regenerate or zero-diff-check
(unlike the Java jextract / C# csbindgen SDKs).

## Building the native library

The Swift package's `CSkopli` target names the C module but does **not**
itself link the native library (the artifact is built out-of-tree by the capi
crate). Build the cdylib for the runner's arch from the workspace
root:

```sh
cargo build -p skopli-capi --release
# macOS  -> target/release/libskopli.dylib  (+ libskopli.a for static)
# Linux  -> target/release/libskopli.so
```

## How CI / macOS verifies this SDK

CI, on a macOS runner, runs from `sdks/swift/`:

```sh
# 1. build the native cdylib (from the workspace root)
cargo build -p skopli-capi --release

# 2. build + test the Swift package, linking the cdylib
swift build \
  -Xlinker -L../../target/release \
  -Xlinker -lskopli

DYLD_LIBRARY_PATH=../../target/release swift test \
  -Xlinker -L../../target/release \
  -Xlinker -lskopli
```

`-Xlinker -L<dir> -Xlinker -lskopli` points the linker at the built
`libskopli.dylib`; `DYLD_LIBRARY_PATH` lets the dynamic loader find it at
run time. On Linux use `LD_LIBRARY_PATH`.

On Windows the Swift toolchain needs `SDKROOT` set and the linker pointed at the
MSVC import lib (`skopli.dll.lib`, not the `.dll`), with the DLL directory on
`PATH` at run time:

```sh
export SDKROOT="$(swiftc -print-target-info | grep -o '"sdkPath": *"[^"]*"' | cut -d'"' -f4)"
swift test \
  -Xlinker -L../../target/release \
  -Xlinker skopli.dll.lib
PATH="../../target/release:$PATH" swift test \
  -Xlinker -L../../target/release \
  -Xlinker skopli.dll.lib
```

To avoid the runtime library search entirely, link the **static** archive
instead (`libskopli.a`); the symbols are baked into the test binary and no
`DYLD_LIBRARY_PATH` is needed:

```sh
swift test -Xlinker -L../../target/release -Xlinker -lskopli \
  -Xlinker -force_load -Xlinker ../../target/release/libskopli.a
```

The smoke test drives read -> rollup -> price against the committed golden case
(`golden/claude/basic`, `golden/pricing/catalogs`) with structural parity vs the
shared conformance gold (the same data the Rust capi and the Go/Java/C# smokes
assert against), plus the flat gpt-5 cost (0.00625), a thrown `.invalidArgument`
(bad `since`), a thrown `.catalog` (unknown catalog format), and the `Pricing`
`deinit`/`close()` free path.

## Usage

```swift
import Skopli

// Detect + read
let detection = try Skopli.detectHarnesses()
let result = try Skopli.readUsage(
    options: ReadUsageOptions(harnesses: [.claude], tz: "UTC"))

// Roll up
let byModel = try Skopli.rollup(events: result.events, by: .model, tz: "UTC")

// Cost a token bundle
let usd = try Skopli.costUSD(
    tokens: TokenCounts(input: 1000, output: 500),
    price: ModelPrice(input: 1.25, output: 10.0))

// Price with injected catalogs (Pricing frees in deinit; close() is optional)
let pricing = try Skopli.createPricing(options: PricingOptions(
    catalogs: [Catalog(source: "litellm", format: "litellm",
                       payload: RawJSON(rawLitellmBytes))]))
let priced = try pricing.priceRollups(rollups: byModel)
let byEvent = try pricing.priceEvents(events: result.events, by: .model)
let hit = try pricing.lookupModel("openai/gpt-5")

// Async conveniences run the sync core off-thread
let result2 = try await Skopli.readUsage(
    options: ReadUsageOptions(harnesses: [.claude]))
```

## Pricing surface

The facade exposes the full pricing surface, at parity with every other Skopli
facade: `lookupModel`, `priceRollups`, `priceEvents`, and `catalogs` on the
`Pricing` class, plus the complete `PricingOptions` set (`sources`, `overrides`,
`mode`, `cacheDir`, `offline`, `ttlMs`, `refresh`, and a `fetch` seam). The
built-in source lifecycle runs facade-side in Swift: built-in sources are on by
default (OpenRouter, then LiteLLM, then models.dev), each cached to disk under
`cacheDir` (defaulting to the shared platform cache dir), served from cache
within the TTL, refreshed on `refresh`, and served stale when offline or when a
fetch fails.

## Pricing sources (facade seam)

The default `libskopli` performs no network I/O (it opens no socket) and has no
callback surface. Fetching and caching live entirely in Swift: implement
`PricingSource` and pass a `fetch: (URL) throws -> Data` closure in
`PricingOptions`; `createPricing` runs each `load` in Swift and hands the
resulting catalog JSON down to the core with `builtinSources: false`. No callback
crosses the FFI.

## Errors

`SkopliError` maps 1:1 from the ABI `AgStatus` codes: `.invalidArgument` (1),
`.catalog` (2), `.internalError` (3), each carrying the thread-local
`ag_last_error_message` detail. The core never throws for malformed _data_ (that
surfaces in `diagnostics`/`skipped`); errors mean a programmer mistake or an
I/O-fatal/internal condition.
