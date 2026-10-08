import type { Code } from "astro:components";
import type { ComponentProps } from "astro/types";

type Lang = ComponentProps<typeof Code>["lang"];

export interface Sample {
  label: string;
  lang: Lang;
  file: string;
  install: string;
  installLang: Lang;
  code: string;
}

export const samples: Sample[] = [
  {
    label: "TypeScript",
    lang: "ts",
    file: "quickstart.ts",
    install: "pnpm add skopli",
    installLang: "sh",
    code: `import { readUsage, rollup, createPricing } from "skopli";

const { events } = await readUsage({ since: "2026-08-01" });
const byModel = rollup(events, { by: "model" });
const priced = await createPricing().priceRollups(byModel);
const total = priced.reduce((t, r) => t + (r.pricing.priced ? r.pricing.usd : 0), 0);`,
  },
  {
    label: "Python",
    lang: "python",
    file: "quickstart.py",
    install: "pip install skopli",
    installLang: "sh",
    code: `import skopli

result = skopli.read_usage(since="2026-08-01")
by_model = skopli.rollup(result.events, "model")
priced = skopli.create_pricing().price_rollups(by_model)
total = sum(r.usd for r in priced if r.pricing.priced)`,
  },
  {
    label: "Rust",
    lang: "rust",
    file: "total.rs",
    install: "cargo add skopli-core",
    installLang: "sh",
    code: `use skopli_core::pricing::{cost_usd, ModelPrice};
use skopli_core::rollup::{rollup, RollupBy, RollupOptions};
use skopli_core::types::UsageEvent;

// skopli-core exposes the primitives: group events, then price each bucket.
fn total(events: &[UsageEvent], price: &ModelPrice) -> f64 {
    rollup(events, &RollupOptions::new(RollupBy::Model))
        .iter()
        .map(|bucket| cost_usd(&bucket.tokens, price))
        .sum()
}`,
  },
  {
    label: "Ruby",
    lang: "ruby",
    file: "quickstart.rb",
    install: "gem install skopli",
    installLang: "sh",
    code: `require "skopli"

result = Skopli.read_usage(since: "2026-08-01")
by_model = Skopli.rollup(result.events, :model)
priced = Skopli.create_pricing.price_rollups(by_model)
total = priced.select { |r| r.pricing.priced }.sum(&:usd)`,
  },
  {
    label: "Java",
    lang: "java",
    file: "Quickstart.java",
    install: `// settings.gradle.kts, with the skopli checkout beside your project
includeBuild("../skopli/sdks/java")

// build.gradle.kts
implementation("com.skopli:skopli")`,
    installLang: "kotlin",
    code: `import com.skopli.*;
import java.util.List;

ReadUsageResult result = Skopli.readUsage(ReadUsageOptions.builder().since("2026-08-01").build());
List<Rollup> byModel = Skopli.rollup(result.events(), RollupOptions.by(RollupBy.MODEL));
try (Pricing pricing = Skopli.createPricing(PricingOptions.builder().build())) {
    List<PricedRollup> priced = pricing.priceRollups(byModel);
    double total = priced.stream().mapToDouble(r -> r.pricing().usd().orElse(0)).sum();
}`,
  },
  {
    label: "C#",
    lang: "csharp",
    file: "Quickstart.cs",
    install: "dotnet add package Skopli",
    installLang: "sh",
    code: `using Skopli;

ReadUsageResult usage = SkopliClient.ReadUsage(new ReadUsageOptions { Since = "2026-08-01" });
IReadOnlyList<Rollup> byModel =
    SkopliClient.Rollup(usage.Events, new RollupOptions { By = RollupBy.Model });
using Pricing pricing = SkopliClient.CreatePricing(new PricingOptions());
IReadOnlyList<PricedRollup> priced = pricing.PriceRollups(byModel);
double total = priced.Sum(r => r.Pricing is PriceHit hit ? hit.Usd : 0);`,
  },
  {
    label: "Go",
    lang: "go",
    file: "main.go",
    install: "go get github.com/skopli/skopli/sdks/go",
    installLang: "sh",
    code: `import "github.com/skopli/skopli/sdks/go/skopli"

result, err := skopli.ReadUsage(skopli.ReadUsageOptions{Since: "2026-08-01"})
if err != nil {
    return err
}
byModel, err := skopli.Rollup(result.Events, skopli.RollupOptions{By: skopli.ByModel})
if err != nil {
    return err
}
pricing, err := skopli.NewPricing(skopli.PricingOptions{})
if err != nil {
    return err
}
defer pricing.Close()
priced, err := pricing.PriceRollups(byModel)
if err != nil {
    return err
}
total := 0.0
for _, r := range priced {
    if r.Pricing.Priced {
        total += *r.Pricing.USD
    }
}`,
  },
  {
    label: "Swift",
    lang: "swift",
    file: "Quickstart.swift",
    install: `// Package.swift, with the skopli checkout beside your project
.package(path: "../skopli/sdks/swift")`,
    installLang: "swift",
    code: `import Skopli

let result = try Skopli.readUsage(options: ReadUsageOptions(since: "2026-08-01"))
let byModel = try Skopli.rollup(events: result.events, by: .model)
let pricing = try Skopli.createPricing()
defer { pricing.close() }
let priced = try pricing.priceRollups(rollups: byModel)
let total = priced.reduce(0) { $0 + ($1.pricing.usd ?? 0) }`,
  },
  {
    label: "C/C++",
    lang: "c",
    file: "rollup.c",
    install: `cargo build -p skopli-capi --release
cc app.c -I crates/skopli-capi/include -L target/release -lskopli`,
    installLang: "sh",
    code: `#include <string.h>
#include "skopli.h"

// The C ABI exchanges JSON buffers. events_json is a UsageEvent[]; the caller
// frees *out with ag_buf_free.
AgStatus rollup_by_model(const char *events_json, uintptr_t events_len, AgBuf *out) {
    const char *opts = "{\\"by\\":\\"model\\"}";
    return ag_rollup(events_json, events_len, opts, strlen(opts), out);
}`,
  },
];
