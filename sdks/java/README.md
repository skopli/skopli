# skopli (Java SDK)

Idiomatic Java bindings for skopli, riding the C ABI (`skopli-capi`) via
the Foreign Function & Memory API (FFM, stable since JDK 22). Typed records and
enums, `Optional`/`OptionalLong` for nullable wire fields, a sealed
`SkopliException` tree (`InvalidArgumentException`, `CatalogException`,
`InternalException`) mapped from the C `AgStatus`, and a sealed `PriceLookup`
(`PriceHit`/`PriceMiss`) for pattern-matching pricing results. Sync and
thread-safe; the pricing handle is `AutoCloseable`.

Requires **JDK 22+** (FFM). The dev/test toolchain is Temurin 25.

## Layout

- `src/main/java/com/skopli/` - the typed facade (pure JDK, no deps).
- `src/main/java/com/skopli/ffi/` - **committed** jextract bindings for the
  17 `ag_*` functions + `AgBuf`/`AgStatus`. The opaque `AgPricing` handle crosses
  as a raw pointer (no struct fields), so its type is intentionally excluded.
- `scripts/regen-bindings.sh` - reproduce the bindings from the C header.
- `scripts/check-bindings.sh` - assert the committed bindings are zero-diff (CI).
- `scripts/smoke.sh` - build + run the smoke test with plain javac/java.

## Building the native library

FFM loads the MSVC `skopli.dll` directly at run time via a symbol lookup - no
import lib and no GNU-target rebuild (unlike the cgo-based Go SDK). Build the
default release cdylib:

```sh
# from the workspace root
cargo build -p skopli-capi --release
```

This produces:

- Windows: `target/release/skopli.dll`
- Linux: `target/release/libskopli.so`
- macOS: `target/release/libskopli.dylib`

## Regenerating the bindings

The bindings are committed, so a normal build needs no jextract. To refresh them
after a C ABI change, point `JEXTRACT` at a jextract 22 launcher (it runs on its
own bundled runtime) and run:

```sh
JEXTRACT=/path/to/jextract-22/bin/jextract bash scripts/regen-bindings.sh
bash scripts/check-bindings.sh   # must report zero-diff
```

## Load mechanics (java.library.path)

The jextract-generated code resolves symbols with `SymbolLookup.loaderLookup()`,
so the cdylib must be loaded into the JVM before the first downcall. The facade's
`NativeLibrary` loader supports two mechanisms:

1. **Explicit file** (recommended, unambiguous): pass the absolute path to the
   cdylib and it is loaded with `System.load`:

   ```sh
   java --enable-native-access=ALL-UNNAMED \
     -Dskopli.library.path=/abs/path/to/target/release/skopli.dll ...
   ```

2. **By name**: otherwise `System.loadLibrary("skopli")` searches
   `-Djava.library.path=<dir>` and the OS loader path (Windows `PATH`,
   Linux `LD_LIBRARY_PATH`, macOS `DYLD_LIBRARY_PATH`). The platform file names
   are `skopli.dll` / `libskopli.so` / `libskopli.dylib`.

`--enable-native-access=ALL-UNNAMED` suppresses the FFM restricted-method warning
(JDK 22-24) / avoids the hard error (future JDKs).

## Test

No build tool is required for the smoke test:

```sh
# JDK 22+ on PATH; cdylib built at target/release
bash scripts/smoke.sh
```

`smoke.sh` compiles the facade + `SmokeTest` with `javac` and runs it with the
load and native-access flags above. It drives read -> sort -> rollup -> price end
to end against the committed conformance gold (`golden/claude/basic`,
`golden/pricing/catalogs`, `golden/pricing/basic`) with structural parity checks,
exercises the sealed `PriceHit`/`PriceMiss` discrimination and the
`AutoCloseable` pricing lifecycle, and forces a `CatalogException`.

The equivalent build-tool entry point is Gradle (`gradle test`, see
`build.gradle.kts`) on a host that has Gradle installed; the test task is
pre-wired with `--enable-native-access` and `-Dskopli.library.path`.

## Pricing surface

The facade exposes the full pricing surface, at parity with every other Skopli
facade: `lookupModel`, `priceRollups`, `priceEvents`, and `catalogs` on the
`Pricing` handle, plus the complete `createPricing` option set (`sources`,
`overrides`, `mode`, `cacheDir`, `offline`, `ttlMs`, `refresh`, and a `fetch`
seam). The built-in source lifecycle runs facade-side in Java: built-in sources
are on by default (OpenRouter, then LiteLLM, then models.dev), each cached to
disk under `cacheDir` (defaulting to the shared platform cache dir), served from
cache within the TTL, refreshed on `refresh`, and served stale when offline or
when a fetch fails.

## Pricing sources (facade seam)

The default `libskopli` performs no network I/O; it opens no socket. Fetching and
caching live entirely in Java: implement `PricingSource` and pass a `Fetcher` in
`PricingOptions`; `Skopli.createPricing` runs each source's `load` in Java and
hands the resulting catalog JSON down to the core (`builtinSources=false`). No
callback crosses the FFI.
