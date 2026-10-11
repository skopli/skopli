// The nine language surfaces. Nothing is published yet, so `install` is the build-from-source line.
export type Language = {
  id: string;
  name: string;
  shiki: string;
  install: string;
  quickstart: string;
};

const native = "cargo build -p skopli-capi --release";

export const languages: Language[] = [
  {
    id: "ts",
    name: "TypeScript",
    shiki: "ts",
    install: "pnpm install && pnpm run build",
    quickstart: `import { detectHarnesses, readUsage, rollup, createPricing } from "skopli";

const { supported, unsupported } = detectHarnesses();
const { events, diagnostics } = await readUsage({ since: "2026-08-01", tz: "UTC" });
const byModel = rollup(events, { by: "model" });

const pricing = createPricing();
for (const row of await pricing.priceRollups(byModel)) {
  if (row.pricing.priced) console.log(row.key, row.pricing.usd.toFixed(2), row.pricing.source);
  else console.log(row.key, "unpriced; tried", row.pricing.attempted.join(", "));
}`,
  },
  {
    id: "py",
    name: "Python",
    shiki: "python",
    install: "maturin develop --release -m crates/skopli-py/Cargo.toml",
    quickstart: `import skopli

detection = skopli.detect_harnesses()
result = skopli.read_usage(since="2026-08-01", tz="UTC")
by_model = skopli.rollup(result.events, by="model")

pricing = skopli.create_pricing()
for row in pricing.price_rollups(by_model):
    if row.pricing.priced:
        print(row.key, round(row.pricing.usd, 2), row.pricing.source)
    else:
        print(row.key, "unpriced; tried", ", ".join(row.pricing.attempted))`,
  },
  {
    id: "rust",
    name: "Rust",
    shiki: "rust",
    install: `# Cargo.toml: skopli-core = { path = "crates/skopli-core" }`,
    quickstart: `use skopli_core::{pricing::{Pricing, PricingMode}, readers::registry::registered_readers, rollup::{rollup, RollupBy, RollupOptions}};

// The core reads and aggregates; the facades fetch catalogs and hand them in.
let mut events = Vec::new();
for reader in registered_readers() {
    events.extend(reader.read(&context)?.events);
}
let by_model = rollup(&events, &RollupOptions::new(RollupBy::Model));

let pricing = Pricing::new(catalogs, None, PricingMode::Auto);
for row in pricing.price_rollups(&by_model) {
    println!("{} {:?}", row.rollup.key, row.pricing);
}`,
  },
  {
    id: "ruby",
    name: "Ruby",
    shiki: "ruby",
    install: "cd crates/skopli-ruby && bundle install && rake compile",
    quickstart: `require "skopli"

result = Skopli.read_usage(since: "2026-08-01", tz: "UTC")
by_model = Skopli.rollup(result.events, :model)

pricing = Skopli.create_pricing
pricing.price_rollups(by_model).each do |row|
  if row.pricing.priced?
    puts "#{row.rollup.key} #{row.usd.round(2)} #{row.pricing.source}"
  else
    puts "#{row.rollup.key} unpriced; tried #{row.pricing.attempted.join(", ")}"
  end
end`,
  },
  {
    id: "java",
    name: "Java",
    shiki: "java",
    install: `${native} && ./gradlew -p sdks/java build`,
    quickstart: `import com.skopli.*;

var result = Skopli.readUsage(new ReadUsageOptions().since("2026-08-01").tz("UTC"));
var byModel = Skopli.rollup(result.events(), new RollupOptions(RollupBy.MODEL));

try (var pricing = Skopli.createPricing(new PricingOptions())) {
  for (var row : pricing.priceRollups(byModel)) {
    System.out.println(row.rollup().key() + " " + row.pricing());
  }
}`,
  },
  {
    id: "csharp",
    name: "C#",
    shiki: "csharp",
    install: `${native} && dotnet build sdks/csharp/Skopli.sln -c Release`,
    quickstart: `using Skopli;

var result = SkopliClient.ReadUsage(new ReadUsageOptions { Since = "2026-08-01", Tz = "UTC" });
var byModel = SkopliClient.Rollup(result.Events, new RollupOptions { By = RollupBy.Model });

using var pricing = SkopliClient.CreatePricing(new PricingOptions());
foreach (var row in pricing.PriceRollups(byModel))
{
    Console.WriteLine($"{row.Rollup.Key} {row.Pricing}");
}`,
  },
  {
    id: "go",
    name: "Go",
    shiki: "go",
    install: `${native} && go build ./sdks/go/...`,
    quickstart: `import "github.com/skopli/skopli/sdks/go/skopli"

result, err := skopli.ReadUsage(skopli.ReadUsageOptions{Since: "2026-08-01", Tz: "UTC"})
byModel, err := skopli.Rollup(result.Events, skopli.RollupOptions{By: skopli.ByModel})

pricing, err := skopli.NewPricing(skopli.PricingOptions{})
defer pricing.Close()
priced, err := pricing.PriceRollups(byModel)
for _, row := range priced {
    fmt.Println(row.Rollup.Key, row.Pricing)
}`,
  },
  {
    id: "swift",
    name: "Swift",
    shiki: "swift",
    install: `${native} && swift build --package-path sdks/swift -Xlinker -L../../target/release -Xlinker -lskopli`,
    quickstart: `import Skopli

let result = try Skopli.readUsage(options: ReadUsageOptions(since: "2026-08-01", tz: "UTC"))
let byModel = try Skopli.rollup(events: result.events, options: RollupOptions(by: .model))

let pricing = try Skopli.createPricing()
defer { pricing.close() }
for row in try pricing.priceRollups(rollups: byModel) {
    print(row.rollup.key, row.pricing)
}`,
  },
  {
    id: "c",
    name: "C",
    shiki: "c",
    install: `${native}  # or --features net for zero-config catalog fetching`,
    quickstart: `#include "skopli.h"

// Every call returns a JSON buffer the caller frees with ag_buf_free.
ag_buf events = ag_read_usage("{\\"since\\":\\"2026-08-01\\",\\"tz\\":\\"UTC\\"}");
ag_buf rollups = ag_rollup(events.ptr, "{\\"by\\":\\"model\\"}");

// libskopli opens no socket: pass catalogs you fetched (or build with --features net).
ag_pricing *pricing = ag_pricing_new(catalogs_json, "{\\"mode\\":\\"auto\\"}");
ag_buf priced = ag_pricing_price_rollups(pricing, rollups.ptr);
ag_pricing_free(pricing);`,
  },
];

export const languageName = (id: string): string => languages.find((l) => l.id === id)?.name ?? id;
