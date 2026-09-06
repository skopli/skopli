package com.skopli.json;

/** Thrown when JSON parsing or serialization fails in the minimal codec. */
public final class JsonException extends RuntimeException {

    private static final long serialVersionUID = 1L;

    public JsonException(String message) {
        super(message);
    }
}
