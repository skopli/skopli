---
title: Getting started
description: Build Skopli from source for your language, confirm which harnesses it can see on this machine, and read your first events.
---

## Before you start

Skopli is pre-launch: no package is published to npm, PyPI, crates.io, RubyGems, Maven Central, NuGet or the Go module proxy yet.
Every language builds from a clone of the repository.
You need Rust 1.88 or newer for the core (the repo pins 1.89.0), plus the toolchain of the language you are calling from.

```sh
git clone https://github.com/skopli/skopli.git
cd skopli
```

## Build

```sh tab=install lang=ts
pnpm install
pnpm run build
```

```sh tab=install lang=py
maturin develop --release -m crates/skopli-py/Cargo.toml
```

```sh tab=install lang=ruby
cd crates/skopli-ruby && bundle install && rake compile
```

```sh tab=install lang=go
cargo build -p skopli-capi --release
go build ./sdks/go/...
```

```sh tab=install lang=java
cargo build -p skopli-capi --release
./gradlew -p sdks/java build
```

```sh tab=install lang=csharp
cargo build -p skopli-capi --release
dotnet build sdks/csharp/Skopli.sln -c Release
```

```sh tab=install lang=swift
cargo build -p skopli-capi --release
swift build --package-path sdks/swift
```

```toml tab=install lang=rust
[dependencies]
skopli-core = { path = "crates/skopli-core", features = ["net"] }
```

```sh tab=install lang=c
cargo build -p skopli-capi --release
```

The TypeScript, Python and Ruby facades embed the core through native bindings; Rust depends on the core crate by path.
Go, Java, C#, Swift and C call `libskopli` through its C ABI, so build `skopli-capi` first (the `sh` lines above) and make the library discoverable to your linker or runtime; C includes `skopli.h` from `crates/skopli-capi/include`.

## Detect

`detectHarnesses` checks the default log location of each of the 40 harnesses and reports which exist on this machine.
It also lists tools whose state was found but whose logs carry no reliable token data: Cursor, aider, Crush and GitLab Duo CLI.

```ts tab=detect
import { detectHarnesses } from "skopli";

const { supported, unsupported } = detectHarnesses();
console.log(supported); // e.g. ["claude", "codex", "opencode"]
console.log(unsupported); // e.g. ["cursor"]
```

```python tab=detect
import skopli

detection = skopli.detect_harnesses()
print(detection.supported, detection.unsupported)
```

```ruby tab=detect
detection = Skopli.detect_harnesses
p detection.supported, detection.unsupported
```

```go tab=detect
detection, err := skopli.DetectHarnesses(nil)
fmt.Println(detection.Supported, detection.Unsupported)
```

```java tab=detect
var detection = Skopli.detectHarnesses(DetectOptions.defaults());
System.out.println(detection.supported() + " " + detection.unsupported());
```

```csharp tab=detect
var detection = SkopliClient.DetectHarnesses();
Console.WriteLine($"{string.Join(",", detection.Supported)} {string.Join(",", detection.Unsupported)}");
```

```swift tab=detect
let detection = try Skopli.detectHarnesses()
print(detection.supported, detection.unsupported)
```

```c tab=detect
AgBuf detection;
if (ag_detect_harnesses(NULL, 0, &detection) != AG_STATUS_OK) return 1;
printf("%.*s\n", (int)detection.len, (const char *)detection.ptr);
ag_buf_free(detection);
```

Detection checks that paths exist.
It does not open or parse log files.

## Read

`readUsage` parses every supported harness it finds and returns three things: the events, a list of diagnostics, and a map of files it skipped.

```ts tab=read
import { readUsage } from "skopli";

const { events, diagnostics, skipped } = await readUsage({
  since: "2026-08-01",
  tz: "Europe/London",
});

console.log(events.length, "events");
for (const d of diagnostics) console.warn(d.severity, d.harness, d.message);
```

```python tab=read
result = skopli.read_usage(since="2026-08-01", tz="Europe/London")
print(len(result.events), "events")
for d in result.diagnostics:
    print(d.severity, d.harness, d.message)
```

```ruby tab=read
result = Skopli.read_usage(since: "2026-08-01", tz: "Europe/London")
puts "#{result.events.size} events"
result.diagnostics.each { |d| warn "#{d.severity} #{d.harness} #{d.message}" }
```

```go tab=read
result, err := skopli.ReadUsage(skopli.ReadUsageOptions{Since: "2026-08-01", TZ: "Europe/London"})
fmt.Println(len(result.Events), "events")
```

```java tab=read
var result = Skopli.readUsage(ReadUsageOptions.builder().since("2026-08-01").tz("Europe/London").build());
System.out.println(result.events().size() + " events");
```

```csharp tab=read
var result = SkopliClient.ReadUsage(new ReadUsageOptions { Since = "2026-08-01", Tz = "Europe/London" });
Console.WriteLine($"{result.Events.Count} events");
```

```swift tab=read
let result = try Skopli.readUsage(options: ReadUsageOptions(since: "2026-08-01", tz: "Europe/London"))
print(result.events.count, "events")
```

```c tab=read
const char *opts = "{\"since\":\"2026-08-01\",\"tz\":\"Europe/London\"}";
AgBuf result;
if (ag_read_usage(opts, strlen(opts), &result) != AG_STATUS_OK) return 1;
printf("%.*s\n", (int)result.len, (const char *)result.ptr);
ag_buf_free(result);
```

The buffer holds the same JSON object the other facades return: `events`, `diagnostics` and `skipped`.

A malformed line does not throw.
It becomes a diagnostic, and a file Skopli could not use at all is listed under `skipped` by harness.
Check both before trusting a total.
An empty result with many diagnostics usually means a permissions problem.

## Next

- [Reading usage](/skopli/guide/reading/): every option and every field of a `UsageEvent`.
- [Rollups](/skopli/guide/rollups/): grouping by model, day, session, harness, workspace or block.
- [Pricing basics](/skopli/guide/pricing-basics/): where prices come from and how to control the network.
