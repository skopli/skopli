package com.skopli.internal;

import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Locates and loads the {@code skopli} cdylib so the jextract-generated
 * {@link com.skopli.ffi.Skopli} downcalls (which use
 * {@code SymbolLookup.loaderLookup()}) can resolve the {@code ag_*} symbols.
 *
 * <p>Resolution order (first hit wins):
 * <ol>
 *   <li>The system property {@code skopli.library.path} - an explicit path
 *       to the shared library file (loaded via {@link System#load}).</li>
 *   <li>{@link System#loadLibrary}{@code ("skopli")} - the JVM searches
 *       {@code java.library.path} (and the OS loader path) for the platform
 *       library name ({@code skopli.dll} / {@code libskopli.so} /
 *       {@code libskopli.dylib}).</li>
 * </ol>
 *
 * <p>Loaded exactly once (class-init idempotent).
 */
public final class NativeLibrary {

    private static volatile boolean loaded;

    private NativeLibrary() {}

    /** Idempotently load the native library, throwing if it cannot be found. */
    public static synchronized void ensureLoaded() {
        if (loaded) {
            return;
        }
        String explicit = System.getProperty("skopli.library.path");
        if (explicit != null && !explicit.isBlank()) {
            Path p = Path.of(explicit);
            if (!Files.isRegularFile(p)) {
                throw new UnsatisfiedLinkError(
                        "skopli.library.path does not point at a file: " + explicit);
            }
            System.load(p.toAbsolutePath().toString());
        } else {
            System.loadLibrary("skopli");
        }
        loaded = true;
    }
}
