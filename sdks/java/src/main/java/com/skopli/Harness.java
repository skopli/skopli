package com.skopli;

/**
 * A supported agent harness. The wire representation is the lowercase id
 * ({@code "claude"}, {@code "opencode"}, ...); {@link #id()} yields it and
 * {@link #fromId(String)} parses it (unknown ids map to {@link #OTHER} so a
 * newer core cannot break an older facade).
 */
public enum Harness {
    CLAUDE("claude"),
    CODEX("codex"),
    GEMINI("gemini"),
    OPENCODE("opencode"),
    MIMOCODE("mimocode"),
    COMMANDCODE("commandcode"),
    COPILOT("copilot"),
    AMP("amp"),
    DROID("droid"),
    QWEN("qwen"),
    PI("pi"),
    OMP("omp"),
    PRIME("prime"),
    GAJAE("gajae"),
    KIMCHI("kimchi"),
    GROK("grok"),
    AUGMENT("augment"),
    CODEBUFF("codebuff"),
    CODEBUDDY("codebuddy"),
    JCODE("jcode"),
    MUX("mux"),
    ZCODE("zcode"),
    ROO("roo"),
    CLINE("cline"),
    KILO("kilo"),
    KILOCODE("kilocode"),
    OPENCLAW("openclaw"),
    KIMI("kimi"),
    JUNIE("junie"),
    DEVIN("devin"),
    HERMES("hermes"),
    GOOSE("goose"),
    ZED("zed"),
    CHERRYSTUDIO("cherrystudio"),
    OPENCODEREVIEW("opencodereview"),
    TRAE("trae"),
    DEEPSEEK("deepseek"),
    REASONIX("reasonix"),
    KIRO("kiro"),
    FX("fx"),
    OTHER("other");

    private final String id;

    Harness(String id) {
        this.id = id;
    }

    /** The wire id (lowercase). */
    public String id() {
        return id;
    }

    /** Parse a wire id; unknown ids yield {@link #OTHER}. */
    public static Harness fromId(String id) {
        for (Harness h : values()) {
            if (h.id.equals(id)) {
                return h;
            }
        }
        return OTHER;
    }
}
