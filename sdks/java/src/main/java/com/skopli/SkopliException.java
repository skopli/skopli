package com.skopli;

/**
 * Base of the unchecked exception tree the facade throws when the C ABI reports
 * a non-zero {@code AgStatus}. Sealed: exactly the three status codes the ABI
 * defines (1 invalid-argument, 2 catalog, 3 internal) map to the three
 * permitted subclasses.
 *
 * <p>Per the seam contract, the core never throws for malformed data
 * (diagnostics/skipped carry it); these exceptions surface only programmer
 * mistakes (bad date/tz, invalid catalog JSON) and fatal internal conditions.
 * The message carries the thread-local {@code ag_last_error_message} detail.
 */
public sealed class SkopliException extends RuntimeException
        permits InvalidArgumentException, CatalogException, InternalException {

    private static final long serialVersionUID = 1L;

    SkopliException(String message) {
        super(message);
    }
}
