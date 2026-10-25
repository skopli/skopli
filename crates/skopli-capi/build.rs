//! Regenerate the committed C header (`include/skopli.h`, cbindgen) and the
//! committed C# P/Invoke bindings (`sdks/csharp/Skopli/Native/NativeMethods.g.cs`,
//! csbindgen) from the crate's `extern "C"` surface.
//!
//! Generation runs only when `SKOPLI_GEN_HEADER=1` or `SKOPLI_GEN_CSHARP=1` is
//! set, which `pnpm run sync` does; a plain `cargo build` stays side-effect free.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=include/skopli.h");
    println!("cargo:rerun-if-env-changed=SKOPLI_GEN_HEADER");
    println!("cargo:rerun-if-env-changed=SKOPLI_GEN_CSHARP");

    if env::var("SKOPLI_GEN_HEADER").as_deref() == Ok("1") {
        gen_header(&crate_dir);
    }

    if env::var("SKOPLI_GEN_CSHARP").as_deref() == Ok("1") {
        gen_csharp(&crate_dir);
    }

    let csharp = csharp_out(&crate_dir);
    if csharp.exists() {
        println!("cargo:rerun-if-changed={}", csharp.display());
    }
}

fn gen_header(crate_dir: &Path) {
    let config =
        cbindgen::Config::from_file(crate_dir.join("cbindgen.toml")).expect("read cbindgen.toml");

    let bindings = cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("generate cbindgen bindings");

    let out = crate_dir.join("include").join("skopli.h");
    std::fs::create_dir_all(out.parent().unwrap()).expect("create include dir");
    bindings.write_to_file(&out);
}

fn csharp_out(crate_dir: &Path) -> PathBuf {
    crate_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .join("sdks/csharp/Skopli/Native/NativeMethods.g.cs")
}

fn gen_csharp(crate_dir: &Path) {
    let out = csharp_out(crate_dir);
    std::fs::create_dir_all(out.parent().unwrap()).expect("create Native dir");

    csbindgen::Builder::default()
        .input_extern_file(crate_dir.join("src").join("lib.rs"))
        .input_extern_file(crate_dir.join("src").join("abi.rs"))
        .csharp_dll_name("skopli")
        .csharp_namespace("Skopli.Native")
        .csharp_class_name("NativeMethods")
        .csharp_class_accessibility("internal")
        .generate_csharp_file(&out)
        .expect("generate csbindgen C# bindings");
}
