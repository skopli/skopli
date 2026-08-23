# skopli (Go SDK)

Idiomatic Go bindings for skopli, riding the C ABI (`skopli-capi`) via
cgo. `(T, error)` returns, PascalCase structs, JSON marshalled to real Go types,
`errors.Is` sentinels (`ErrInvalidArgument`, `ErrCatalog`, `ErrInternal`).
Sync and goroutine-safe.

## Building the native library

cgo drives the MinGW/GNU `gcc`/`ld` toolchain on Windows, which links against a
GNU import lib - NOT the MSVC `skopli.dll.lib`. Build the capi cdylib for the
GNU target:

```sh
# from the workspace root
cargo build -p skopli-capi --release --target x86_64-pc-windows-gnu
```

This produces `target/x86_64-pc-windows-gnu/release/skopli.dll` and the GNU
import lib `libskopli.dll.a`. On non-Windows, build the default target
(`cargo build -p skopli-capi --release`) to get `target/release/`.

## cgo link + load mechanics

`cabi.go` declares the compile/link directives against the workspace layout:

```
#cgo CFLAGS: -I${SRCDIR}/../../../crates/skopli-capi/include
#cgo windows LDFLAGS: -L${SRCDIR}/../../../target/x86_64-pc-windows-gnu/release -lskopli
#cgo !windows LDFLAGS: -L${SRCDIR}/../../../target/release -lskopli
```

For an out-of-tree layout, override with environment variables, e.g.:

```sh
export CGO_CFLAGS="-I/path/to/skopli/include"
export CGO_LDFLAGS="-L/path/to/libdir -lskopli"
```

At **run time** the DLL directory must be on the loader path:

```sh
# Windows (bash): put the GNU-target release dir on PATH
DLL=/c/.../skopli/target/x86_64-pc-windows-gnu/release
PATH="$DLL:$PATH" go test ./...

# Linux/macOS: LD_LIBRARY_PATH / DYLD_LIBRARY_PATH -> target/release
```

## Test

```sh
DLL=/c/.../skopli/target/x86_64-pc-windows-gnu/release
PATH="$DLL:$PATH" go test ./...
```

The smoke test drives read -> rollup -> price against the committed golden case
(`golden/claude/basic`, `golden/pricing/catalogs`) with structural parity vs the
shared conformance gold, and verifies the `errors.Is` sentinel mapping.

## Pricing surface

The facade exposes the full pricing surface, at parity with every other Skopli
facade: `LookupModel`, `PriceRollups`, `PriceEvents`, and `Catalogs` on the
`Pricing` handle, plus the complete `PricingOptions` set (`Sources`, `Overrides`,
`Mode`, `CacheDir`, `Offline`, `TTLMs`, `Refresh`, and a `Fetch` seam). The
built-in source lifecycle runs facade-side in Go: built-in sources are on by
default (OpenRouter, then LiteLLM, then models.dev), each cached to disk under
`CacheDir` (defaulting to the shared platform cache dir), served from cache
within the TTL, refreshed on `Refresh`, and served stale when offline or when a
fetch fails.

## Pricing sources (facade seam)

The default `libskopli` performs no network I/O; it opens no socket. Fetching and
caching live entirely in Go: implement `PricingSource` and pass a `Fetch` func in
`PricingOptions`; `NewPricing` runs each `Load` in Go and hands the resulting
catalog JSON down to the core (`builtinSources` is always sent false). No
callback crosses the FFI.
