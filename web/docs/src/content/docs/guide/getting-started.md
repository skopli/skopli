---
title: Getting started
description: Build Skopli from source for your language, confirm which harnesses it can see on this machine, and read your first events.
---

## Before you start

Skopli is pre-launch: no package is published to npm, PyPI, crates.io, RubyGems, Maven Central, NuGet or the Go module proxy yet.
Every language builds from a clone of the repository.
You need Rust 1.88 or newer for the core, plus the toolchain of the language you are calling from.

```sh
git clone https://github.com/skopli/skopli.git
cd skopli
```

## Build

```sh tab=install
pnpm install
pnpm run build
```

```sh tab=install
# Python
maturin develop --release -m crates/skopli-py/Cargo.toml
```

```sh tab=install
# Ruby
cd crates/skopli-ruby && bundle install && rake compile
```

```sh tab=install
# Go: the facade links against libskopli
cargo build -p skopli-capi --release
go build ./sdks/go/...
```

```sh tab=install
# Java
cargo build -p skopli-capi --release
./gradlew -p sdks/java build
```

```sh tab=install
# C#
cargo build -p skopli-capi --release
dotnet build sdks/csharp/Skopli.sln -c Release
```

```sh tab=install
# Swift
cargo build -p skopli-capi --release
swift build --package-path sdks/swift
```

```sh tab=install
# Rust: depend on the core crate by path
# [dependencies]
# skopli-core = { path = "crates/skopli-core" }
```

```sh tab=install
# C: build libskopli and include skopli.h
cargo build -p skopli-capi --release
```

The TypeScript, Python and Ruby facades embed the core through native bindings.
Go, Java, C#, Swift and C call `libskopli` through its C ABI, so build `skopli-capi` first and make the library discoverable to your linker or runtime.

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
detection, err := skopli.DetectHarnesses()
fmt.Println(detection.Supported, detection.Unsupported)
```

```java tab=detect
var detection = Skopli.detectHarnesses();
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
ag_buf detection = ag_detect_harnesses("{}");
printf("%.*s\n", (int)detection.len, detection.ptr);
ag_buf_free(detection);
```

Detection reads directory listings only.
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
result, err := skopli.ReadUsage(skopli.ReadUsageOptions{Since: "2026-08-01", Tz: "Europe/London"})
fmt.Println(len(result.Events), "events")
```

```java tab=read
var result = Skopli.readUsage(new ReadUsageOptions().since("2026-08-01").tz("Europe/London"));
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
ag_buf result = ag_read_usage("{\"since\":\"2026-08-01\",\"tz\":\"Europe/London\"}");
/* result.ptr is JSON: {"events":[...],"diagnostics":[...],"skipped":{...}} */
ag_buf_free(result);
```

A malformed line does not throw.
It becomes a diagnostic, and a file Skopli could not use at all is listed under `skipped` by harness.
Check both before trusting a total. An empty result with a dozen diagnostics is a permissions problem, not a quiet month.

## Next

- [Reading usage](/skopli/guide/reading/): every option and every field of a `UsageEvent`.
- [Rollups](/skopli/guide/rollups/): grouping by model, day, session, harness, workspace or block.
- [Pricing basics](/skopli/guide/pricing-basics/): where prices come from and how to control the network.
