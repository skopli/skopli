# skopli (Ruby)

Native Ruby binding over `skopli-core` (via [magnus] + [rb-sys]) - read,
roll up, and price AI coding-agent token usage. The gem is a thin, idiomatic
Ruby facade (`Data.define` value classes, symbol dimensions like `:claude` /
`:day`, keyword-args, the `Skopli::Error` hierarchy) over a native extension
that binds the Rust core **directly** (not the C ABI).

Sync-only v1: every native call releases the GVL around the Rust work, so other
Ruby threads make progress.

## Layout

This gem is crate-local: the same directory is both a Cargo crate
(`crates/skopli-ruby`) and a RubyGem.

```
crates/skopli-ruby/
  Cargo.toml            # cdylib crate `skopli` (magnus + rb-sys)
  src/                  # Rust: lib.rs (magnus binding) + gvl/modules/parse/
                        #       pipeline/pricing_opts/wire (JSON boundary, shared
                        #       with skopli-py)
  skopli.gemspec    # crate-local gemspec
  Rakefile              # rake compile (rb_sys/extensiontask) + rake test
  ext/skopli/extconf.rb  # rb-sys create_rust_makefile
  lib/skopli.rb     # facade module functions
  lib/skopli/       # errors, value classes, Pricing
  test/test_smoke.rb    # read -> rollup -> price against golden/claude/basic
```

## Build & test

Requires Ruby 3.2+ and a Rust toolchain.

```sh
bundle install
rake compile   # builds the native extension via cargo
rake test      # runs the smoke test
# or:
gem build skopli.gemspec
```

## Usage

```ruby
require "skopli"

result = Skopli.read_usage(harnesses: [:claude], tz: "UTC")
rollups = Skopli.rollup(result.events, :model, tz: "UTC")

pricing = Skopli.create_pricing(
  overrides: [{ model: "claude-sonnet-4-5", input: 3.0, output: 15.0 }]
)
priced = pricing.price_rollups(rollups)
priced.each { |p| puts "#{p.rollup.key}: $#{p.usd}" }
```

[magnus]: https://github.com/matsadler/magnus
[rb-sys]: https://github.com/oxidize-rb/rb-sys
