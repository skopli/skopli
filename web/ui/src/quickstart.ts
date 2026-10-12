import type { Lang } from "./langs.ts";

export type Quickstart = {
  id: Lang;
  registry?: string;
  install?: string;
  source: string;
  note?: string;
  code: string;
};

export const published = false;

export const releaseNote = published
  ? ""
  : "No release has shipped yet, and no registry has a usable package. Each line below installs from the repository.";

export const command = (q: Quickstart) => (published ? (q.install ?? q.source) : q.source);

export const quickstarts: Quickstart[] = [
  {
    id: "ts",
    registry: "npm",
    install: "pnpm add skopli",
    source:
      "git clone https://github.com/skopli/skopli && pnpm --dir skopli install && pnpm --dir skopli run build && pnpm add ./skopli",
    note: "Node 24+, ESM only. Building from source needs a Rust toolchain.",
    code: `import { detectHarnesses, readUsage, rollup, createPricing } from "skopli";

const { supported, unsupported } = detectHarnesses();
console.log(supported, unsupported);

const { events, diagnostics } = await readUsage({
  harnesses: ["claude", "opencode", "codex"],
  since: "2026-08-01",
});

const byModel = rollup(events, { by: "model" });

const pricing = createPricing();
const priced = await pricing.priceRollups(byModel);

for (const row of priced) {
  if (row.pricing.priced) {
    console.log(row.key, row.pricing.usd, row.pricing.source);
  } else {
    console.log(row.key, "unpriced, tried", row.pricing.attempted);
  }
}`,
  },
  {
    id: "python",
    registry: "PyPI",
    install: "pip install skopli",
    source: 'pip install "git+https://github.com/skopli/skopli#subdirectory=crates/skopli-py"',
    note: "Python 3.10+. Building from source needs a Rust toolchain.",
    code: `import skopli

detection = skopli.detect_harnesses()
print(detection.supported, detection.unsupported)
result = skopli.read_usage(
    harnesses=["claude", "opencode", "codex"], since="2026-08-01"
)
rollups = skopli.rollup(result.events, by="model")

pricing = skopli.create_pricing()
for row in pricing.price_rollups(rollups):
    if row.pricing.priced:
        print(row.rollup.key, row.usd, row.pricing.source)
    else:
        print(row.rollup.key, "unpriced, tried", row.pricing.attempted)`,
  },
  {
    id: "rust",
    registry: "crates.io",
    install: "cargo add skopli-core",
    source: "cargo add skopli-core --git https://github.com/skopli/skopli",
    note: "Rust 1.88+.",
    code: `use skopli_core::pricing::parse::parse_litellm;
use skopli_core::pricing::types::PricingCatalog;
use skopli_core::pricing::{Pricing, PricingMode};
use skopli_core::rollup::{rollup, RollupBy, RollupOptions};

// \`events\` is a Vec<UsageEvent> you already hold. \`litellm_json\` is a
// serde_json::Value you fetched yourself. Without the \`net\` feature the core crate fetches nothing.
let by_model = rollup(&events, &RollupOptions::new(RollupBy::Model));

let catalog = PricingCatalog {
    source: "litellm".into(),
    fetched_at: None,
    prices: parse_litellm(&litellm_json),
};
let pricing = Pricing::new(vec![catalog], None, PricingMode::Calculate);
let priced = pricing.price_rollups(&by_model);`,
  },
  {
    id: "ruby",
    registry: "RubyGems",
    install: "gem install skopli",
    source:
      'gem "skopli", git: "https://github.com/skopli/skopli", glob: "crates/skopli-ruby/*.gemspec"',
    note: "Ruby 3.2+. Building from source needs a Rust toolchain.",
    code: `require "skopli"

result = Skopli.read_usage(
  harnesses: [:claude, :opencode, :codex], since: "2026-08-01"
)
rollups = Skopli.rollup(result.events, :model)

pricing = Skopli.create_pricing
pricing.price_rollups(rollups).each do |row|
  puts "#{row.rollup.key}: #{row.usd.inspect}"
end`,
  },
  {
    id: "java",
    registry: "GitHub Releases",
    install: 'gh release download --repo skopli/skopli --pattern "skopli-[0-9]*[0-9].jar"',
    source:
      "git clone https://github.com/skopli/skopli && cargo build --manifest-path skopli/Cargo.toml -p skopli-capi --release && gradle -p skopli/sdks/java jar",
    note: "JDK 22+. sdks/java/README.md covers the jar and the run-time flags.",
    code: `import com.skopli.*;

ReadUsageOptions opts = ReadUsageOptions.builder().since("2026-08-01").build();
ReadUsageResult usage = Skopli.readUsage(opts);
RollupOptions byModelOpts = RollupOptions.by(RollupBy.MODEL);
List<Rollup> byModel = Skopli.rollup(usage.events(), byModelOpts);

try (Pricing pricing = Skopli.createPricing(PricingOptions.builder().build())) {
    List<PricedRollup> priced = pricing.priceRollups(byModel);
}`,
  },
  {
    id: "csharp",
    registry: "NuGet",
    install: "dotnet add package Skopli",
    source:
      "git clone https://github.com/skopli/skopli && cargo build --manifest-path skopli/Cargo.toml -p skopli-capi --release && dotnet add reference skopli/sdks/csharp/Skopli/Skopli.csproj",
    note: ".NET 9. sdks/csharp/README.md covers how libskopli is found.",
    code: `using Skopli;

var readOpts = new ReadUsageOptions { Since = "2026-08-01" };
ReadUsageResult usage = SkopliClient.ReadUsage(readOpts);
var rollupOpts = new RollupOptions { By = RollupBy.Model };
IReadOnlyList<Rollup> byModel = SkopliClient.Rollup(usage.Events, rollupOpts);

using Pricing pricing = SkopliClient.CreatePricing(new PricingOptions());
IReadOnlyList<PricedRollup> priced = pricing.PriceRollups(byModel);`,
  },
  {
    id: "go",
    registry: "Go modules",
    install: "go get github.com/skopli/skopli/sdks/go",
    source:
      "git clone https://github.com/skopli/skopli && cargo build --manifest-path skopli/Cargo.toml -p skopli-capi --release && go mod edit -replace github.com/skopli/skopli/sdks/go=./skopli/sdks/go && go get github.com/skopli/skopli/sdks/go",
    note: "Go 1.26+. Needs cgo and a C compiler. sdks/go/README.md covers out-of-tree and Windows builds.",
    code: `import "github.com/skopli/skopli/sdks/go/skopli"

func priceByModel() ([]skopli.PricedRollup, error) {
	usage, err := skopli.ReadUsage(skopli.ReadUsageOptions{Since: "2026-08-01"})
	if err != nil {
		return nil, err
	}
	opts := skopli.RollupOptions{By: skopli.ByModel}
	byModel, err := skopli.Rollup(usage.Events, opts)
	if err != nil {
		return nil, err
	}
	pricing, err := skopli.NewPricing(skopli.PricingOptions{})
	if err != nil {
		return nil, err
	}
	defer pricing.Close()
	return pricing.PriceRollups(byModel)
}`,
  },
  {
    id: "swift",
    source:
      "git clone https://github.com/skopli/skopli && cargo build --manifest-path skopli/Cargo.toml -p skopli-capi --release",
    note: "macOS 13+, Linux, or Windows. sdks/swift/README.md covers the path dependency and the link flags.",
    code: `import Skopli

let result = try Skopli.readUsage(options: ReadUsageOptions(since: "2026-08-01"))
let byModel = try Skopli.rollup(events: result.events, by: .model)

let pricing = try Skopli.createPricing(options: PricingOptions())
let priced = try pricing.priceRollups(rollups: byModel)`,
  },
  {
    id: "c",
    registry: "GitHub Releases",
    install:
      'gh release download --repo skopli/skopli --pattern "skopli-capi-*-x86_64-unknown-linux-gnu.tar.gz"',
    source:
      "git clone https://github.com/skopli/skopli && cargo build --manifest-path skopli/Cargo.toml -p skopli-capi --release",
    note: "Each release attaches one archive per target with skopli.h and the shared and static libraries, for x86_64 and aarch64 Linux, macOS, and Windows.",
    code: `#include "skopli.h"

// Every call takes and returns JSON. Free each AgBuf with ag_buf_free.
// The prebuilt library has no network layer, so pass a catalog you fetched.
const char *read_opts = "{\\"since\\":\\"2026-08-01\\"}";
AgBuf usage = {0};
if (ag_read_usage(read_opts, strlen(read_opts), &usage) != AG_STATUS_OK)
  return 1;

// ag_rollup takes the "events" array from the usage JSON above; pull it out
// with your JSON library into events_json / events_len.
const char *rollup_opts = "{\\"by\\":\\"model\\"}";
AgBuf by_model = {0};
if (ag_rollup(events_json, events_len, rollup_opts, strlen(rollup_opts),
              &by_model) != AG_STATUS_OK)
  return 1;

const char *pricing_opts =
    "{\\"builtinSources\\":false,\\"catalogs\\":[{\\"source\\":\\"litellm\\","
    "\\"format\\":\\"litellm\\",\\"payload\\":" LITELLM_JSON "}]}";
AgPricing *pricing = NULL;
if (ag_pricing_new(pricing_opts, strlen(pricing_opts), &pricing)
    != AG_STATUS_OK)
  return 1;

AgBuf priced = {0};
ag_pricing_price_rollups(pricing, (const char *)by_model.ptr, by_model.len,
                         &priced);

ag_pricing_free(pricing);
ag_buf_free(priced);
ag_buf_free(by_model);
ag_buf_free(usage);`,
  },
];
