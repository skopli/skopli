package com.skopli;

/** {@code AgStatus == 1}: a caller-supplied argument was invalid (null where
 * required, malformed JSON, out-of-range date/tz, or a bad enum value). */
public final class InvalidArgumentException extends SkopliException {

    private static final long serialVersionUID = 1L;

    public InvalidArgumentException(String message) {
        super(message);
    }
}
