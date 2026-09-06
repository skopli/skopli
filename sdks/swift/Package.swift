// swift-tools-version:5.9
import PackageDescription

// skopli Swift SDK - a hand-written idiomatic facade over the skopli C
// ABI (crates/skopli-capi). NOT UniFFI: a small SwiftPM package over the
// committed C header stays idiomatic and adds zero new toolchain.
//
// Two targets:
//   - CSkopli: a `systemLibrary` target that imports the C header via a
//     module map (Sources/CSkopli/module.modulemap -> the committed
//     crates/skopli-capi/include/skopli.h). It only surfaces the `ag_*`
//     symbols; it does NOT itself link the native library.
//   - Skopli: the pure-Swift idiomatic facade (Codable models, throws, a
//     Pricing final class, the pricing-source seam) that depends on CSkopli.
//
// LINKER NOTE (how CI/macOS links the native cdylib/staticlib):
//   The systemLibrary target names the module but leaves linking to the build
//   invocation, because the native artifact is built out-of-tree (the capi
//   crate). CI builds the cdylib/staticlib and points swift at it:
//
//     # from the workspace root, build the capi cdylib for the runner arch
//     cargo build -p skopli-capi --release
//     # -> target/release/libskopli.dylib (+ libskopli.a for static)
//
//     # then build/test the Swift package, pointing the linker at it:
//     swift build \
//       -Xlinker -L../../target/release \
//       -Xlinker -lskopli
//     swift test \
//       -Xlinker -L../../target/release \
//       -Xlinker -lskopli
//
//   At run time the dynamic loader must find the dylib
//   (DYLD_LIBRARY_PATH=../../target/release on macOS, LD_LIBRARY_PATH on Linux),
//   OR link the staticlib (`-lskopli` against libskopli.a) to avoid the
//   runtime search entirely. See README.md for the full CI recipe.
let package = Package(
    name: "Skopli",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .library(name: "Skopli", targets: ["Skopli"])
    ],
    targets: [
        .systemLibrary(
            name: "CSkopli",
            path: "Sources/CSkopli"
        ),
        .target(
            name: "Skopli",
            dependencies: ["CSkopli"],
            path: "Sources/Skopli"
        ),
        .testTarget(
            name: "SkopliTests",
            dependencies: ["Skopli"],
            path: "Tests/SkopliTests"
        ),
    ]
)
