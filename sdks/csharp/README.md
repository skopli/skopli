# skopli - C#/.NET SDK

An idiomatic C# facade over the skopli C ABI. P/Invoke bindings are generated
by [csbindgen](https://github.com/Cysharp/csbindgen) from the `skopli-capi`
crate and wrapped in a typed facade: `record` types, enums, exceptions, and
`System.Text.Json` marshalling of the JSON boundary.

## Layout

```
sdks/csharp/
  Skopli.sln
  Skopli/                     class library (the SDK)
    Skopli.csproj             assembly name Skopli.Sdk (avoids the
                                  skopli.dll native-file collision)
    SkopliClient.cs                      static entry: detect/read/rollup/cost/pricing
    Pricing.cs                    IDisposable pricing handle
    Models.cs / PriceLookup.cs    record data model + hit/miss union
    Options.cs                    option records + PricingSource seam
    Exceptions.cs                 SkopliException hierarchy
    Native/
      NativeMethods.g.cs          csbindgen-generated P/Invoke (COMMITTED)
      Interop.cs                  the only class touching P/Invoke (ownership +
                                  AgStatus -> exception mapping)
      NativeLibraryResolver.cs    resolves the native cdylib
  Skopli.Tests/               xunit smoke/unit tests
```

## Native library resolution

The P/Invoke layer imports the native library by the bare name `skopli`. A
`NativeLibrary.SetDllImportResolver` hook (`NativeLibraryResolver`) resolves it,
in order:

1. `SKOPLI_LIBRARY_PATH` env var (a full file path), if set;
2. `runtimes/<rid>/native/<lib>` beside the assembly (e.g.
   `runtimes/win-x64/native/skopli.dll`) - the standard NuGet native layout;
3. the assembly directory itself;
4. the OS loader search path (PATH / rpath / DllImport default).

The platform file name is `skopli.dll` (Windows), `libskopli.so`
(Linux), `libskopli.dylib` (macOS). The managed assembly is
`Skopli.Sdk.dll`, so it never collides with the native `skopli.dll` on
case-insensitive filesystems.

## Building the native library

```sh
cargo build -p skopli-capi --release
# -> target/release/skopli.dll (+ .lib import lib, unused by .NET)
```

.NET P/Invoke loads the MSVC DLL directly at runtime - no import lib and no
GNU-target rebuild are needed (unlike Go/cgo).

## Build & test

```sh
dotnet build sdks/csharp/Skopli.sln -c Release
dotnet test  sdks/csharp/Skopli.sln -c Release
```

The test project copies `target/release/skopli.dll` into
`bin/<cfg>/net9.0/runtimes/win-x64/native/` after build (override the source with
the `NativeLib` MSBuild property or `SKOPLI_LIBRARY_PATH`).

## Regenerating the P/Invoke bindings

`NativeMethods.g.cs` is committed and CI-checked for a zero-diff against a fresh
csbindgen run of the crate's `extern "C"` surface:

```sh
SKOPLI_GEN_CSHARP=1 cargo build -p skopli-capi   # regenerate
scripts/check-csharp-bindings.sh                          # zero-diff gate
```

Generation is gated on the env var so ordinary `cargo build` stays unaffected.

## API

```csharp
using Skopli;

// detect / read / rollup / cost
Detection d = SkopliClient.DetectHarnesses();
ReadUsageResult usage = SkopliClient.ReadUsage(new ReadUsageOptions { Tz = "UTC" });
IReadOnlyList<Rollup> byModel = SkopliClient.Rollup(usage.Events, new RollupOptions { By = RollupBy.Model });
double usd = SkopliClient.CostUsd(tokens, price);

// pricing (IDisposable) - full surface: LookupModel, PriceRollups, PriceEvents, Catalogs
using Pricing pricing = SkopliClient.CreatePricing(new PricingOptions {
    Mode = PricingMode.Calculate,
    Catalogs = new[] { new Catalog { Source = "litellm", Format = "litellm", Payload = json } },
});
IReadOnlyList<PricedRollup> priced = pricing.PriceRollups(byModel);
PriceLookup hit = pricing.LookupModel("openai/gpt-5");

// async conveniences run the sync core off-thread
ReadUsageResult u = await SkopliClient.ReadUsageAsync(new ReadUsageOptions { });
```

### Pricing surface

The facade exposes the full pricing surface, at parity with every other Skopli
facade: `LookupModel`, `PriceRollups`, `PriceEvents`, and `Catalogs` on the
`Pricing` handle, plus the complete `CreatePricing` option set (`Sources`,
`Overrides`, `Mode`, `CacheDir`, `Offline`, `TtlMs`, `Refresh`, and a `Fetch`
seam). The built-in source lifecycle runs facade-side in C#: built-in sources
are on by default (OpenRouter, then LiteLLM, then models.dev), each cached to
disk under `CacheDir` (defaulting to the shared platform cache dir), served from
cache within the TTL, refreshed on `Refresh`, and served stale when offline or
when a fetch fails.

### Pricing sources (facade-side seam)

The default `libskopli` opens no socket and has no callback surface. Fetching and
caching run entirely in C#; the resulting catalog JSON is handed down to the core
with `builtinSources: false`. A source is a `PricingSource` record (`Name`, `Url`,
and a core parser `Format` of `"openrouter"`, `"litellm"`, or `"modelsdev"`), and
`Fetch` is a `Func<string, Task<string>>` returning the raw source JSON as text:

```csharp
using var p = SkopliClient.CreatePricing(new PricingOptions {
    Sources = new[] {
        PricingSource.OpenRouter(),
        new PricingSource { Name = "my-source", Url = "https://example/prices.json", Format = "litellm" },
    },
    Fetch = async url => await new HttpClient().GetStringAsync(url),
});
```

### Errors

`SkopliException` is the base; `InvalidArgumentException` (bad date/tz/JSON),
`CatalogException` (pricing/catalog failures), and `InternalException` map the
ABI's `AgStatus` codes. Malformed usage data never throws - it rides
`ReadUsageResult.Diagnostics` / `.Skipped`.
