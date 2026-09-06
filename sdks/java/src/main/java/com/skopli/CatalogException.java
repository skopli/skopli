package com.skopli;

/** {@code AgStatus == 2}: a pricing/catalog operation failed (e.g. unparseable
 * catalog JSON). */
public final class CatalogException extends SkopliException {

    private static final long serialVersionUID = 1L;

    public CatalogException(String message) {
        super(message);
    }
}
