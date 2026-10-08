import type { Code } from "astro:components";
import type { ComponentProps } from "astro/types";

type Lang = ComponentProps<typeof Code>["lang"];

export interface Sample {
  label: string;
  lang: Lang;
  file: string;
  install: string;
  installLang: Lang;
  installFile?: string;
  installWhen?: "afterRelease" | "fromCheckout";
  code: string;
}

export const samples: Sample[] = [
  {
    label: "TypeScript",
    lang: "ts",
    file: "quickstart.ts",
    install: "pnpm add skopli",
    installLang: "sh",
    installWhen: "afterRelease",
    code: `import { readUsage, rollup, createPricing } from "skopli";

const { events } = await readUsage({ since: "2026-08-01" });
const byModel = rollup(events, { by: "model" });
const priced = await createPricing().priceRollups(byModel);
const total = priced.reduce(
  (sum, row) => sum + (row.pricing.priced ? row.pricing.usd : 0),
  0,
);`,
  },
  {
    label: "Python",
    lang: "python",
    file: "quickstart.py",
    install: "pip install skopli",
    installLang: "sh",
    installWhen: "afterRelease",
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
    installWhen: "afterRelease",
    code: `use skopli_core::pricing::parse::parse_openrouter;
use skopli_core::pricing::{Pricing, PricingCatalog, PricingMode, RollupPricing};
use skopli_core::rollup::{rollup, RollupBy, RollupOptions};
use skopli_core::types::UsageEvent;

// skopli-core exposes the primitives and fetches nothing: group events, then
// price each bucket against a catalog you fetched (the raw OpenRouter /models JSON).
fn total(events: &[UsageEvent], openrouter: &serde_json::Value) -> f64 {
    let catalog = PricingCatalog {
        source: "openrouter".into(),
        fetched_at: None,
        prices: parse_openrouter(openrouter),
    };
    let pricing = Pricing::new(vec![catalog], None, PricingMode::Calculate);
    let by_model = rollup(events, &RollupOptions::new(RollupBy::Model));
    pricing
        .price_rollups(&by_model)
        .iter()
        .map(|row| match &row.pricing {
            RollupPricing::Hit { usd, .. } => *usd,
            _ => 0.0,
        })
        .sum()
}`,
  },
  {
    label: "Ruby",
    lang: "ruby",
    file: "quickstart.rb",
    install: "gem install skopli",
    installLang: "sh",
    installWhen: "afterRelease",
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
    installFile: "settings.gradle.kts and build.gradle.kts",
    code: `import com.skopli.*;
import java.util.List;

ReadUsageResult result = Skopli.readUsage(
    ReadUsageOptions.builder().since("2026-08-01").build());
List<Rollup> byModel = Skopli.rollup(
    result.events(), RollupOptions.by(RollupBy.MODEL));
try (Pricing pricing = Skopli.createPricing(PricingOptions.builder().build())) {
    List<PricedRollup> priced = pricing.priceRollups(byModel);
    double total = priced.stream()
        .mapToDouble(r -> r.pricing().usd().orElse(0))
        .sum();
}`,
  },
  {
    label: "C#",
    lang: "csharp",
    file: "Quickstart.cs",
    install: "dotnet add package Skopli.Sdk",
    installLang: "sh",
    installWhen: "afterRelease",
    code: `using Skopli;

ReadUsageResult usage = SkopliClient.ReadUsage(
    new ReadUsageOptions { Since = "2026-08-01" });
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
    installWhen: "afterRelease",
    code: `import "github.com/skopli/skopli/sdks/go/skopli"

result, err := skopli.ReadUsage(
    skopli.ReadUsageOptions{Since: "2026-08-01"})
if err != nil {
    return err
}
byModel, err := skopli.Rollup(
    result.Events, skopli.RollupOptions{By: skopli.ByModel})
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
    installFile: "Package.swift",
    code: `import Skopli

let result = try Skopli.readUsage(options: ReadUsageOptions(since: "2026-08-01"))
let byModel = try Skopli.rollup(events: result.events, by: .model)
let pricing = try Skopli.createPricing()
defer { pricing.close() }
let priced = try pricing.priceRollups(rollups: byModel)
let total = priced.reduce(0) { $0 + ($1.pricing.usd ?? 0) }`,
  },
  {
    label: "C",
    lang: "c",
    file: "total.c",
    install: `cargo build -p skopli-capi --release
cc app.c -I crates/skopli-capi/include -L target/release -lskopli`,
    installLang: "sh",
    installWhen: "fromCheckout",
    code: `#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "skopli.h"

// The C ABI exchanges JSON buffers. The default libskopli fetches nothing, so
// pass a catalog you fetched yourself: here the raw OpenRouter /models JSON.
AgStatus price_by_model(const char *events_json, uintptr_t events_len,
                        const char *openrouter_json, AgBuf *out) {
    const char *by_model = "{\\"by\\":\\"model\\"}";
    AgBuf rollups = {0};
    AgStatus st = ag_rollup(events_json, events_len, by_model, strlen(by_model), &rollups);
    if (st != AG_STATUS_OK) return st;

    const char *head = "{\\"builtinSources\\":false,\\"catalogs\\":[{\\"source\\":\\"openrouter\\","
                       "\\"format\\":\\"openrouter\\",\\"payload\\":";
    char *opts = malloc(strlen(head) + strlen(openrouter_json) + 4);
    if (!opts) {
        ag_buf_free(rollups);
        return AG_STATUS_INTERNAL;
    }
    size_t len = (size_t)sprintf(opts, "%s%s}]}", head, openrouter_json);
    AgPricing *pricing = NULL;
    st = ag_pricing_new(opts, len, &pricing);
    if (st == AG_STATUS_OK)
        st = ag_pricing_price_rollups(pricing, (const char *)rollups.ptr, rollups.len, out);
    ag_pricing_free(pricing);
    ag_buf_free(rollups);
    free(opts);
    return st;
}`,
  },
];
