# frozen_string_literal: true

require_relative "lib/skopli/version"

Gem::Specification.new do |spec|
  spec.name = "skopli"
  spec.version = Skopli::VERSION
  spec.summary = "Read, roll up, and price AI coding-agent token usage."
  spec.description =
    "Native (magnus / rb-sys) Ruby binding over skopli-core: read agent " \
    "session usage, roll it up, and price it. Sync API; the GVL is released " \
    "around the Rust work."
  spec.authors = ["skopli"]
  spec.license = "MIT"
  spec.homepage = "https://docs.skopli.com/skopli/"
  spec.metadata = {
    "homepage_uri" => "https://docs.skopli.com/skopli/",
    "documentation_uri" => "https://docs.skopli.com/skopli/",
    "source_code_uri" => "https://github.com/skopli/skopli"
  }

  # Ruby 3.2+ for Data.define value classes.
  spec.required_ruby_version = ">= 3.2.0"

  spec.files = Dir[
    "lib/**/*.rb",
    "src/**/*.rs",
    "extconf.rb",
    "Cargo.toml",
    "Cargo.lock",
    "README.md"
  ]
  spec.require_paths = ["lib"]

  # The rb-sys native extension (compiled from this crate root).
  spec.extensions = ["extconf.rb"]

  # rb_sys builds the Rust cdylib as the native extension at install time.
  spec.add_dependency "rb_sys", "~> 0.9"

  spec.add_development_dependency "rake", "~> 13.0"
  spec.add_development_dependency "rake-compiler", "~> 1.2"
  spec.add_development_dependency "minitest", "~> 5.0"
end
