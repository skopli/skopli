package com.skopli;

/** {@code AgStatus == 3}: an unexpected internal error, including a caught panic
 * on the native side. */
public final class InternalException extends SkopliException {

    private static final long serialVersionUID = 1L;

    public InternalException(String message) {
        super(message);
    }
}
