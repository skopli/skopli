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
by_model = skopli.rollup(result.events, "model")

pricing = skopli.create_pricing()
for row in pricing.price_rollups(by_model):
    if row.pricing.priced:
        print(row.rollup.key, round(row.usd, 2), row.pricing.source)
    else:
        print(row.rollup.key, "unpriced; tried", ", ".join(row.pricing.attempted))`,
  },
  {
    id: "rust",
    name: "Rust",
    shiki: "rust",
    install: `skopli-core = { path = "crates/skopli-core" }`,
    quickstart: `use skopli_core::pricing::{Pricing, PricingMode};
use skopli_core::readers::{registered_readers, ReaderContext};
use skopli_core::rollup::{rollup, RollupBy, RollupOptions};

let ctx = ReaderContext::new(std::env::var("HOME")?);
let mut events = Vec::new();
for reader in registered_readers() {
    events.extend(reader.read(&ctx).events);
}
let by_model = rollup(&events, &RollupOptions::new(RollupBy::Model));

let pricing = Pricing::new(catalogs, None, PricingMode::Calculate);
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

detection = Skopli.detect_harnesses
result = Skopli.read_usage(since: "2026-08-01", tz: "UTC")
by_model = Skopli.rollup(result.events, :model)

pricing = Skopli.create_pricing
pricing.price_rollups(by_model).each do |row|
  if row.pricing.priced
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

var detection = Skopli.detectHarnesses(DetectOptions.defaults());
var result = Skopli.readUsage(ReadUsageOptions.builder().since("2026-08-01").tz("UTC").build());
var byModel = Skopli.rollup(result.events(), RollupOptions.by(RollupBy.MODEL));

try (var pricing = Skopli.createPricing(PricingOptions.builder().build())) {
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

var detection = SkopliClient.DetectHarnesses();
var result = SkopliClient.ReadUsage(new ReadUsageOptions { Since = "2026-08-01", Tz = "UTC" });
var byModel = SkopliClient.Rollup(result.Events, new RollupOptions { By = RollupBy.Model });

using var pricing = SkopliClient.CreatePricing(new PricingOptions());
foreach (var row in pricing.PriceRollups(byModel))
{
    Console.WriteLine($"{row.Key} {row.Pricing}");
}`,
  },
  {
    id: "go",
    name: "Go",
    shiki: "go",
    install: `${native} && go build ./sdks/go/...`,
    quickstart: `import "github.com/skopli/skopli/sdks/go/skopli"

detection, err := skopli.DetectHarnesses(nil)
result, err := skopli.ReadUsage(skopli.ReadUsageOptions{Since: "2026-08-01", TZ: "UTC"})
byModel, err := skopli.Rollup(result.Events, skopli.RollupOptions{By: skopli.ByModel})

pricing, err := skopli.NewPricing(skopli.PricingOptions{})
defer pricing.Close()
priced, err := pricing.PriceRollups(byModel)
for _, row := range priced {
    fmt.Println(row.Key, row.Pricing.Priced, row.Pricing.Source, row.Pricing.Attempted)
}`,
  },
  {
    id: "swift",
    name: "Swift",
    shiki: "swift",
    install: `${native} && swift build --package-path sdks/swift`,
    quickstart: `import Skopli

let detection = try Skopli.detectHarnesses()
let result = try Skopli.readUsage(options: ReadUsageOptions(since: "2026-08-01", tz: "UTC"))
let byModel = try Skopli.rollup(events: result.events, by: .model)

let pricing = try Skopli.createPricing()
defer { pricing.close() }
for row in try pricing.priceRollups(rollups: byModel) {
    print(row.key, row.pricing.priced, row.pricing.usd ?? 0, row.pricing.attempted ?? [])
}`,
  },
  {
    id: "c",
    name: "C",
    shiki: "c",
    install: `${native}  # add --features net to fetch catalogs from inside the library`,
    quickstart: `#include "skopli.h"

const char *read_opts = "{\\"since\\":\\"2026-08-01\\",\\"tz\\":\\"UTC\\"}";
const char *by_model = "{\\"by\\":\\"model\\"}";
AgBuf events, rollups, priced;
AgPricing *pricing = NULL;

if (ag_read_usage(read_opts, strlen(read_opts), &events) != AG_STATUS_OK) return 1;
ag_rollup((const char *)events.ptr, events.len, by_model, strlen(by_model), &rollups);
ag_pricing_new(pricing_opts, strlen(pricing_opts), &pricing);
ag_pricing_price_rollups(pricing, (const char *)rollups.ptr, rollups.len, &priced);

ag_pricing_free(pricing);
ag_buf_free(events);
ag_buf_free(rollups);
ag_buf_free(priced);`,
  },
];

export const languageName = (id: string): string => languages.find((l) => l.id === id)?.name ?? id;
