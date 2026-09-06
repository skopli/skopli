# frozen_string_literal: true

# rb-sys extconf: build the Rust cdylib crate as the native extension.
#
# This file lives at the crate root (== the gem root == the Cargo manifest
# directory), which is where rb-sys's ExtensionTask expects it. `create_rust_makefile`
# generates a Makefile that shells out to `cargo build` and installs the resulting
# cdylib (named `skopli` via [lib] name in Cargo.toml) as
# `skopli/skopli.<dlext>`, which the pure-Ruby facade `require`s.

require "mkmf"
require "rb_sys/mkmf"

create_rust_makefile("skopli/skopli")
