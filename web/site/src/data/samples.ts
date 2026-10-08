export interface Sample {
  label: string;
  lang: string;
  file: string;
  install: string;
  installLang: string;
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
    file: "main.rs",
    install: "cargo add skopli-core",
    installLang: "sh",
    code: `use skopli_core::pricing::{cost_usd, ModelPrice};
use skopli_core::rollup::{rollup, RollupBy, RollupOptions};
use skopli_core::types::TokenCounts;

let by_model = rollup(&events, &RollupOptions::new(RollupBy::Model));
let tokens = TokenCounts { input: 1_000, output: 500, ..Default::default() };
let usd = cost_usd(&tokens, &ModelPrice::flat(1.25, 10.0));`,
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
    install: `implementation("com.skopli:skopli")`,
    installLang: "groovy",
    code: `import com.skopli.*;
import java.util.List;

ReadUsageResult result = Skopli.readUsage(ReadUsageOptions.builder().since("2026-08-01").build());
List<Rollup> byModel = Skopli.rollup(result.events(), RollupOptions.by(RollupBy.MODEL));
try (Pricing pricing = Skopli.createPricing(PricingOptions.builder().build())) {
    List<PricedRollup> priced = pricing.priceRollups(byModel);
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
IReadOnlyList<PricedRollup> priced = pricing.PriceRollups(byModel);`,
  },
  {
    label: "Go",
    lang: "go",
    file: "main.go",
    install: "go get github.com/skopli/skopli/sdks/go",
    installLang: "sh",
    code: `import "github.com/skopli/skopli/sdks/go/skopli"

result, _ := skopli.ReadUsage(skopli.ReadUsageOptions{Since: "2026-08-01"})
byModel, _ := skopli.Rollup(result.Events, skopli.RollupOptions{By: skopli.ByModel})
pricing, _ := skopli.NewPricing(skopli.PricingOptions{})
defer pricing.Close()
priced, _ := pricing.PriceRollups(byModel)`,
  },
  {
    label: "Swift",
    lang: "swift",
    file: "Quickstart.swift",
    install: `.package(url: "https://github.com/skopli/skopli", from: "1.0.0")`,
    installLang: "swift",
    code: `import Skopli

let result = try Skopli.readUsage(options: ReadUsageOptions(since: "2026-08-01"))
let byModel = try Skopli.rollup(events: result.events, by: .model)
let pricing = try Skopli.createPricing()
defer { pricing.close() }
let priced = try pricing.priceRollups(rollups: byModel)`,
  },
  {
    label: "C/C++",
    lang: "c",
    file: "quickstart.c",
    install: "cc app.c -lskopli",
    installLang: "sh",
    code: `#include <string.h>
#include "skopli.h"

// events_json / events_len are a serialized UsageEvent[] from ag_read_usage.
AgBuf out = {0};
const char *opts = "{\\"by\\":\\"model\\"}";
AgStatus st = ag_rollup(events_json, events_len, opts, strlen(opts), &out);
if (st == AG_STATUS_OK) ag_buf_free(out);`,
  },
];
