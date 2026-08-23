package com.skopli;

import com.skopli.internal.Wire;
import com.skopli.json.Json;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Map;

/**
 * Standalone smoke test for the Java SDK, driving the facade end-to-end through
 * FFM against the committed conformance gold (the same data the Rust capi and Go
 * SDK smoke tests use). No JUnit dependency (no build tool present): a
 * {@code main} that runs each check and exits non-zero on failure.
 *
 * <p>Run with {@code --enable-native-access=ALL-UNNAMED} and the cdylib on the
 * loader path (see the README); the repo root is passed as {@code args[0]} (or
 * discovered relative to the working directory).
 */
public final class SmokeTest {

    private static int checks;

    public static void main(String[] args) {
        Path repoRoot = args.length > 0 ? Path.of(args[0]) : discoverRepoRoot();
        System.out.println("repo root: " + repoRoot.toAbsolutePath());

        Skopli.load();

        testMeta();
        testHarnessEnumCoversRegistry(repoRoot);
        testCostUsdFlat();
        testDetectFindsClaude(repoRoot);
        testReadRollupMatchesGold(repoRoot);
        testReadOpencodereviewGolden(repoRoot);
        testReadTraeGolden(repoRoot);
        testReadCherryStudioGolden(repoRoot);
        testReadDeepseekGolden(repoRoot);
        testReadReasonixGolden(repoRoot);
        testRollupTzConformance(repoRoot);
        testRollupBlockConformance(repoRoot);
        testRollupBlockOmittedWidthUsesDefault();
        testPricingRoundtripMatchesGold(repoRoot);
        testPriceRollupsMatchesGold(repoRoot);
        testLookupModelHitAndMiss(repoRoot);
        testLifecycleMatrix(repoRoot);
        testLifecycleConstantsMatchGolden(repoRoot);
        testProbeConformance(repoRoot);
        testTimestampConformance(repoRoot);
        testPricingCloses(repoRoot);
        testForcedCatalogError();

        System.out.println("ALL " + checks + " CHECKS PASSED");
    }

    // -- meta / cost ------------------------------------------------------

    private static void testMeta() {
        assertEq(1, Skopli.abiVersion(), "abiVersion");
        assertEq(1, Skopli.schemaVersion(), "schemaVersion");
        assertTrue(!Skopli.version().isEmpty(), "version non-empty");
        assertTrue(Skopli.defaultCacheDir().contains("skopli"), "cache dir mentions skopli");
    }

    /**
     * Assert the public {@link Harness} enum covers every id in the shared
     * registry-id fixture ({@code golden/registry/ids.json}). Each id must parse
     * to a concrete constant (never {@link Harness#OTHER}), so a reader added to
     * the core cannot silently collapse to the fallback in the Java surface.
     */
    private static void testHarnessEnumCoversRegistry(Path repoRoot) {
        Object fixture = readJson(repoRoot.resolve("golden/registry/ids.json"));
        Map<String, Object> obj = Wire.asObject(fixture);
        java.util.Set<String> registered = new java.util.HashSet<>();
        for (Object node : (List<?>) obj.get("ids")) {
            String id = Wire.asString(node);
            registered.add(id);
            assertTrue(Harness.fromId(id) != Harness.OTHER,
                    "registry: Harness enum covers id " + id);
        }
        for (Harness h : Harness.values()) {
            if (h == Harness.OTHER) {
                continue;
            }
            assertTrue(registered.contains(h.id()),
                    "registry: enum id " + h.id() + " exists in registry");
        }
    }

    private static void testCostUsdFlat() {
        TokenCounts tokens = TokenCounts.of(1000, 500, 0, 0, 0);
        ModelPrice price = ModelPrice.flat(1.25, 10.0);
        double usd = Skopli.costUsd(tokens, price);
        double expected = (1000.0 * 1.25 + 500.0 * 10.0) / 1_000_000.0;
        assertTrue(Math.abs(usd - expected) < 1e-12, "costUsd flat = " + usd);
    }

    // -- detect / read / rollup vs claude gold ----------------------------

    private static ReadUsageOptions claudeOptions(Path repoRoot) {
        Path inputDir = repoRoot.resolve("golden").resolve("claude").resolve("basic").resolve("input");
        return ReadUsageOptions.builder()
                .home("/nonexistent")
                .env("CLAUDE_CONFIG_DIR", inputDir.toString())
                .harness(Harness.CLAUDE)
                .tz("UTC")
                .build();
    }

    private static void testDetectFindsClaude(Path repoRoot) {
        ReadUsageOptions ru = claudeOptions(repoRoot);
        DetectOptions detect = DetectOptions.builder()
                .home("/nonexistent")
                .env("CLAUDE_CONFIG_DIR",
                        repoRoot.resolve("golden/claude/basic/input").toString())
                .build();
        Detection detection = Skopli.detectHarnesses(detect);
        assertTrue(detection.supported().contains(Harness.CLAUDE), "claude detected");
        assertTrue(detection.unsupported().isEmpty(), "unsupported empty over this ABI");
    }

    private static void testReadRollupMatchesGold(Path repoRoot) {
        ReadUsageResult result = Skopli.readUsage(claudeOptions(repoRoot));
        assertTrue(!result.events().isEmpty(), "read some events");

        // Sort events the way the exporter/gold does before rolling up.
        List<UsageEvent> sorted = new ArrayList<>(result.events());
        sorted.sort(Comparator
                .comparing(UsageEvent::timestamp)
                .thenComparing(UsageEvent::sessionId)
                .thenComparing(UsageEvent::messageId)
                .thenComparing(UsageEvent::model));

        List<Rollup> byModel = Skopli.rollup(sorted, RollupOptions.by(RollupBy.MODEL, "UTC"));

        // Structural parity vs the committed gold's model lane.
        Object gold = readJson(repoRoot.resolve("golden/claude/basic/expected-rollup.json"));
        // Re-serialize both to normalized JSON via the facade round-trip and compare.
        List<Object> actualWire = new ArrayList<>();
        for (Rollup r : byModel) {
            actualWire.add(rollupToComparable(r));
        }
        List<Object> goldWire = new ArrayList<>();
        for (Object o : (List<?>) Wire.asObject(Wire.asObject(gold).get("by")).get("model")) {
            goldWire.add(normalizeRollup(Wire.asObject(o)));
        }
        assertEq(Json.write(goldWire), Json.write(actualWire), "rollup(by=model) == gold model lane");
    }

    // -- new-reader goldens through the production read path ---------------

    /**
     * Drive {@code golden/opencodereview/basic} through the facade's own
     * {@link Skopli#readUsage} (home-anchored) and assert discriminating values:
     * per-event cache splits, a model that falls back to the session-start
     * model, and the ignored {@code session_end} summary (four events, not the
     * inflated rollup).
     */
    private static void testReadOpencodereviewGolden(Path repoRoot) {
        Path inputDir =
                repoRoot.resolve("golden").resolve("opencodereview").resolve("basic").resolve("input");
        ReadUsageOptions opts = ReadUsageOptions.builder()
                .home(inputDir.toString())
                .harness(Harness.OPENCODEREVIEW)
                .tz("UTC")
                .build();
        ReadUsageResult result = Skopli.readUsage(opts);

        List<UsageEvent> events = new ArrayList<>(result.events());
        events.sort(Comparator
                .comparing(UsageEvent::timestamp)
                .thenComparing(UsageEvent::sessionId)
                .thenComparing(UsageEvent::messageId));

        assertEq(4, events.size(), "opencodereview/basic: four events (summary ignored)");
        assertTrue(events.stream().allMatch(e -> e.harness().equals("opencodereview")),
                "opencodereview/basic: every event is the opencodereview harness");

        UsageEvent first = events.get(0);
        assertEq("ocr-sonnet", first.model(), "opencodereview/basic: first model");
        assertEq(100L, first.tokens().input(), "opencodereview/basic: first input");
        assertEq(50L, first.tokens().output(), "opencodereview/basic: first output");
        assertEq(300L, first.tokens().cacheRead(), "opencodereview/basic: first cacheRead");
        assertEq(200L, first.tokens().cacheWrite(), "opencodereview/basic: first cacheWrite");

        UsageEvent second = events.get(1);
        assertEq(600L, second.tokens().cacheRead(), "opencodereview/basic: second cacheRead split");
        assertEq(0L, second.tokens().cacheWrite(), "opencodereview/basic: second cacheWrite split");

        UsageEvent last = events.get(3);
        assertEq("ocr-haiku", last.model(), "opencodereview/basic: second-session model");
        assertEq(900L, last.tokens().cacheWrite(), "opencodereview/basic: second-session cacheWrite");
    }

    /**
     * Drive {@code golden/trae/basic} through the facade's own
     * {@link Skopli#readUsage} with an INJECTED {@code cwd} (trae discovers
     * trajectories under {@code <cwd>/trajectories}) and assert discriminating
     * values: cache-creation/read and reasoning mapping, a response-model that
     * falls back to the trajectory model, and the ignored {@code agent_steps}
     * (four events, not the inflated step numbers). This also exercises the
     * F1 cwd forwarding contract through the Java options envelope.
     */
    private static void testReadTraeGolden(Path repoRoot) {
        Path cwd = repoRoot.resolve("golden").resolve("trae").resolve("basic").resolve("input");
        ReadUsageOptions opts = ReadUsageOptions.builder()
                .home("/nonexistent")
                .cwd(cwd.toString())
                .harness(Harness.TRAE)
                .tz("UTC")
                .build();
        ReadUsageResult result = Skopli.readUsage(opts);

        List<UsageEvent> events = new ArrayList<>(result.events());
        events.sort(Comparator
                .comparing(UsageEvent::timestamp)
                .thenComparing(UsageEvent::sessionId)
                .thenComparing(UsageEvent::messageId));

        assertEq(4, events.size(), "trae/basic: four interactions (agent_steps ignored)");
        assertTrue(events.stream().allMatch(e -> e.harness().equals("trae")),
                "trae/basic: every event is the trae harness");

        UsageEvent first = events.get(0);
        assertEq("trae-sonnet", first.model(), "trae/basic: first model");
        assertEq(100L, first.tokens().input(), "trae/basic: first input");
        assertEq(50L, first.tokens().output(), "trae/basic: first output");
        assertEq(300L, first.tokens().cacheRead(), "trae/basic: first cacheRead");
        assertEq(200L, first.tokens().cacheWrite(), "trae/basic: first cacheWrite");
        assertEq(40L, first.tokens().reasoning(), "trae/basic: first reasoning");

        UsageEvent third = events.get(2);
        assertEq("traj-default", third.model(), "trae/basic: response model falls back to trajectory model");

        UsageEvent last = events.get(3);
        assertEq("trae-haiku", last.model(), "trae/basic: second-file model");
        assertEq(900L, last.tokens().cacheWrite(), "trae/basic: second-file cacheWrite");
    }

    /**
     * Drive {@code golden/cherrystudio/basic} through the facade's own
     * {@link Skopli#readUsage} with an INJECTED home (Cherry Studio reads the
     * single {@code cherrystudio.sqlite} database under
     * {@code $HOME/.config/CherryStudio/Data}) and assert the two surviving row
     * kinds: a per-invocation row with cache splits and reasoning, and a
     * legacy-aggregate row. The all-zero row is dropped, and the epoch-ms
     * {@code created_at} is converted to ISO.
     */
    private static void testReadCherryStudioGolden(Path repoRoot) {
        Path home =
                repoRoot.resolve("golden").resolve("cherrystudio").resolve("basic").resolve("input");
        ReadUsageOptions opts = ReadUsageOptions.builder()
                .home(home.toString())
                .harness(Harness.CHERRYSTUDIO)
                .tz("UTC")
                .build();
        ReadUsageResult result = Skopli.readUsage(opts);

        List<UsageEvent> events = new ArrayList<>(result.events());
        events.sort(Comparator
                .comparing(UsageEvent::timestamp)
                .thenComparing(UsageEvent::sessionId)
                .thenComparing(UsageEvent::messageId));

        assertEq(2, events.size(), "cherrystudio/basic: two events (zero-token row dropped)");
        assertTrue(events.stream().allMatch(e -> e.harness().equals("cherrystudio")),
                "cherrystudio/basic: every event is the cherrystudio harness");

        UsageEvent agg = events.get(0);
        assertEq("agg1", agg.sessionId(), "cherrystudio/basic: legacy-aggregate survives");
        assertEq("openai/gpt-4o", agg.model(), "cherrystudio/basic: aggregate model");
        assertEq(1000L, agg.tokens().input(), "cherrystudio/basic: aggregate input");
        assertEq(500L, agg.tokens().output(), "cherrystudio/basic: aggregate output");
        assertEq("2025-06-15T15:06:40.000Z", agg.timestamp(),
                "cherrystudio/basic: aggregate epoch-ms converted to ISO");

        UsageEvent inv = events.get(1);
        assertEq("inv1", inv.sessionId(), "cherrystudio/basic: invocation survives");
        assertEq("anthropic/claude-sonnet-4-5", inv.model(), "cherrystudio/basic: invocation model");
        assertEq(100L, inv.tokens().input(), "cherrystudio/basic: invocation input");
        assertEq(40L, inv.tokens().output(), "cherrystudio/basic: invocation output");
        assertEq(20L, inv.tokens().cacheRead(), "cherrystudio/basic: invocation cacheRead");
        assertEq(8L, inv.tokens().cacheWrite(), "cherrystudio/basic: invocation cacheWrite");
        assertEq(10L, inv.tokens().reasoning(), "cherrystudio/basic: invocation reasoning");
        assertEq("2025-08-01T10:00:00.000Z", inv.timestamp(),
                "cherrystudio/basic: invocation epoch-ms converted to ISO");
    }

    /**
     * Drive {@code golden/deepseek/basic} through the facade's own
     * {@link Skopli#readUsage} with an INJECTED {@code DSH_HOME} env override
     * (DeepSeek Harness walks {@code $DSH_HOME/**\/session.jsonl[.zstd]}) and
     * assert discriminating values: a Zstandard-compressed session is read, the
     * early {@code assistant/chunk} usage sample is ignored (not double counted),
     * the zero-token message is dropped, a subagent session is tagged, and the
     * truncated {@code .zstd} file yields a file-level malformed diagnostic.
     */
    private static void testReadDeepseekGolden(Path repoRoot) {
        Path inputDir =
                repoRoot.resolve("golden").resolve("deepseek").resolve("basic").resolve("input");
        ReadUsageOptions opts = ReadUsageOptions.builder()
                .home("/nonexistent")
                .env("DSH_HOME", inputDir.toString())
                .harness(Harness.DEEPSEEK)
                .tz("UTC")
                .build();
        ReadUsageResult result = Skopli.readUsage(opts);

        List<UsageEvent> events = new ArrayList<>(result.events());
        events.sort(Comparator
                .comparing(UsageEvent::timestamp)
                .thenComparing(UsageEvent::sessionId)
                .thenComparing(UsageEvent::messageId));

        assertEq(4, events.size(), "deepseek/basic: four events (chunk + zero-token dropped)");
        assertTrue(events.stream().allMatch(e -> e.harness().equals("deepseek")),
                "deepseek/basic: every event is the deepseek harness");

        UsageEvent first = events.get(0);
        assertEq("deepseek-chat", first.model(), "deepseek/basic: first model");
        assertEq(100L, first.tokens().input(), "deepseek/basic: first input (chunk ignored)");
        assertEq(50L, first.tokens().output(), "deepseek/basic: first output");
        assertEq(30L, first.tokens().cacheRead(), "deepseek/basic: first cacheRead");
        assertEq(20L, first.tokens().cacheWrite(), "deepseek/basic: first cacheWrite");
        assertEq(7L, first.tokens().reasoning(), "deepseek/basic: first reasoning");

        UsageEvent subagent = events.stream()
                .filter(e -> e.sessionId().equals("sess-sub"))
                .findFirst()
                .orElseThrow();
        assertTrue(subagent.subagent(), "deepseek/basic: sess-sub is a subagent session");
        assertEq(200L, subagent.tokens().input(), "deepseek/basic: subagent input");

        UsageEvent zstd = events.stream()
                .filter(e -> e.sessionId().equals("sess-zstd"))
                .findFirst()
                .orElseThrow();
        assertEq(5L, zstd.tokens().input(), "deepseek/basic: zstd session decompressed and read");

        assertTrue(
                result.diagnostics().stream().anyMatch(d -> d.message().contains("sess-bad")),
                "deepseek/basic: truncated .zstd yields a file-level malformed diagnostic");
    }

    /**
     * Drive {@code golden/reasonix/basic} through the facade's own
     * {@link Skopli#readUsage} with an INJECTED {@code REASONIX_HOME} env
     * override (Reasonix walks {@code $REASONIX_HOME/projects/*\/sessions/*.acp.json})
     * and assert discriminating values: one event per session from the cumulative
     * total, {@code input = cacheMissTokens} and {@code output = completion -
     * reasoning}, an estimated session still included, and a session id shared by
     * two sidecars deduped to the later {@code updatedAt} snapshot.
     */
    private static void testReadReasonixGolden(Path repoRoot) {
        Path inputDir =
                repoRoot.resolve("golden").resolve("reasonix").resolve("basic").resolve("input");
        ReadUsageOptions opts = ReadUsageOptions.builder()
                .home("/nonexistent")
                .env("REASONIX_HOME", inputDir.toString())
                .harness(Harness.REASONIX)
                .tz("UTC")
                .build();
        ReadUsageResult result = Skopli.readUsage(opts);

        List<UsageEvent> events = new ArrayList<>(result.events());
        events.sort(Comparator
                .comparing(UsageEvent::timestamp)
                .thenComparing(UsageEvent::sessionId)
                .thenComparing(UsageEvent::messageId));

        assertEq(4, events.size(), "reasonix/basic: one event per session id (dedup)");
        assertTrue(events.stream().allMatch(e -> e.harness().equals("reasonix")),
                "reasonix/basic: every event is the reasonix harness");

        UsageEvent sessA = events.stream()
                .filter(e -> e.sessionId().equals("sess-a"))
                .findFirst()
                .orElseThrow();
        assertEq(70L, sessA.tokens().input(), "reasonix/basic: input = cacheMissTokens");
        assertEq(40L, sessA.tokens().output(), "reasonix/basic: output = completion - reasoning");
        assertEq(30L, sessA.tokens().cacheRead(), "reasonix/basic: cacheRead = cacheHitTokens");
        assertEq(0L, sessA.tokens().cacheWrite(), "reasonix/basic: no cacheWrite persisted");
        assertEq(10L, sessA.tokens().reasoning(), "reasonix/basic: reasoning");

        UsageEvent sessB = events.stream()
                .filter(e -> e.sessionId().equals("sess-b"))
                .findFirst()
                .orElseThrow();
        assertEq(800L, sessB.tokens().input(), "reasonix/basic: estimated session is still included");

        UsageEvent sessC = events.stream()
                .filter(e -> e.sessionId().equals("sess-c"))
                .findFirst()
                .orElseThrow();
        assertEq(0L, sessC.tokens().input(),
                "reasonix/basic: absent cache-miss split yields input 0, not promptTokens");
        assertEq(100L, sessC.tokens().output(), "reasonix/basic: output = completion - reasoning");
        assertEq(500L, sessC.tokens().cacheRead(), "reasonix/basic: cacheRead = cacheHitTokens");

        UsageEvent dup = events.stream()
                .filter(e -> e.sessionId().equals("dup"))
                .findFirst()
                .orElseThrow();
        assertEq(400L, dup.tokens().input(),
                "reasonix/basic: latest updatedAt instant wins across offset spellings");
    }

    // -- timezone day-bucketing vs shared gold ----------------------------

    /**
     * Timezone day-bucketing conformance, driven by the SHARED fixtures in
     * {@code golden/rollup-tz/}. Each case rolls one synthetic event per
     * timestamp up by day in the case's zone and asserts the resulting buckets
     * equal the shared gold, exercising the same native core the Rust and
     * other-language suites use.
     */
    private static void testRollupTzConformance(Path repoRoot) {
        Object cases = readJson(repoRoot.resolve("golden/rollup-tz/cases.json"));
        for (Object node : (List<?>) cases) {
            Map<String, Object> kase = Wire.asObject(node);
            String name = Wire.asString(kase.get("name"));
            String tz = Wire.asString(kase.get("tz"));

            List<UsageEvent> events = new ArrayList<>();
            for (Object ts : (List<?>) kase.get("timestamps")) {
                events.add(new UsageEvent("h", Wire.asString(ts), "s", "m", true, false,
                        "m", TokenCounts.of(0, 0, 0, 0, 0),
                        java.util.OptionalLong.empty(), java.util.OptionalDouble.empty(),
                        java.util.Optional.empty(), java.util.Optional.empty()));
            }

            List<Rollup> got = Skopli.rollup(events, RollupOptions.by(RollupBy.DAY, tz));

            List<Object> expected = new ArrayList<>();
            for (Object b : (List<?>) kase.get("expected")) {
                Map<String, Object> bucket = Wire.asObject(b);
                expected.add(Map.of(
                        "key", Wire.asString(bucket.get("key")),
                        "events", ((Number) bucket.get("events")).longValue()));
            }
            List<Object> actual = new ArrayList<>();
            for (Rollup r : got) {
                actual.add(Map.of("key", r.key(), "events", r.events()));
            }
            assertEq(Json.write(expected), Json.write(actual), "rollup-tz: " + name);
        }
    }

    // -- billing-block windowing vs shared gold ---------------------------

    /**
     * Billing-block windowing conformance, driven by the SHARED fixtures in
     * {@code golden/rollup-block/}. Each case rolls one synthetic event per
     * timestamp into blocks of the case's width in the case's zone and asserts
     * the resulting blocks equal the shared gold, exercising the same native
     * core the Rust and other-language suites use.
     */
    private static void testRollupBlockConformance(Path repoRoot) {
        Object cases = readJson(repoRoot.resolve("golden/rollup-block/cases.json"));
        for (Object node : (List<?>) cases) {
            Map<String, Object> kase = Wire.asObject(node);
            String name = Wire.asString(kase.get("name"));
            String tz = Wire.asString(kase.get("tz"));
            long blockMs = ((Number) kase.get("blockMs")).longValue();

            List<UsageEvent> events = new ArrayList<>();
            for (Object ts : (List<?>) kase.get("timestamps")) {
                events.add(new UsageEvent("h", Wire.asString(ts), "s", "m", true, false,
                        "m", TokenCounts.of(0, 0, 0, 0, 0),
                        java.util.OptionalLong.empty(), java.util.OptionalDouble.empty(),
                        java.util.Optional.empty(), java.util.Optional.empty()));
            }

            List<Rollup> got = Skopli.rollup(events, RollupOptions.block(blockMs, tz));

            List<Object> expected = new ArrayList<>();
            for (Object b : (List<?>) kase.get("expected")) {
                Map<String, Object> block = Wire.asObject(b);
                expected.add(Map.of(
                        "key", Wire.asString(block.get("key")),
                        "events", ((Number) block.get("events")).longValue()));
            }
            List<Object> actual = new ArrayList<>();
            for (Rollup r : got) {
                actual.add(Map.of("key", r.key(), "events", r.events()));
            }
            assertEq(Json.write(expected), Json.write(actual), "rollup-block: " + name);
        }
    }

    /**
     * Omitting {@code blockMs} (an empty optional the wire builder drops) must
     * apply the five-hour default: two events 3h41m apart join one block
     * anchored to 09:00, and an event past five hours opens a new block. Guards
     * the facade's absent-optional serialization, not the fixture's width.
     */
    private static void testRollupBlockOmittedWidthUsesDefault() {
        RollupOptions omitted =
                new RollupOptions(RollupBy.BLOCK, java.util.Optional.of("UTC"), java.util.Optional.empty());

        List<Rollup> joined = Skopli.rollup(
                List.of(blockEvent("2026-01-01T09:17:00.000Z"), blockEvent("2026-01-01T13:00:00.000Z")),
                omitted);
        assertEq(
                Json.write(List.of(Map.of("key", "2026-01-01T09:00:00.000Z", "events", 2L))),
                Json.write(blockKeys(joined)),
                "rollup-block omitted-width: within five hours joins");

        List<Rollup> split = Skopli.rollup(
                List.of(blockEvent("2026-01-01T09:00:00.000Z"), blockEvent("2026-01-01T14:30:00.000Z")),
                omitted);
        assertEq(
                Json.write(List.of(
                        Map.of("key", "2026-01-01T09:00:00.000Z", "events", 1L),
                        Map.of("key", "2026-01-01T14:00:00.000Z", "events", 1L))),
                Json.write(blockKeys(split)),
                "rollup-block omitted-width: past five hours splits");
    }

    private static UsageEvent blockEvent(String ts) {
        return new UsageEvent("h", ts, "s", "m", true, false, "m", TokenCounts.of(0, 0, 0, 0, 0),
                java.util.OptionalLong.empty(), java.util.OptionalDouble.empty(),
                java.util.Optional.empty(), java.util.Optional.empty());
    }

    private static List<Object> blockKeys(List<Rollup> rollups) {
        List<Object> actual = new ArrayList<>();
        for (Rollup r : rollups) {
            actual.add(Map.of("key", r.key(), "events", r.events()));
        }
        return actual;
    }

    // -- pricing round-trip vs expected-priced-rollup.json ----------------

    /** The exact synthetic events from the pricing conformance case (mirrors the
     * capi/Go smoke tests). */
    private static List<UsageEvent> pricingEvents() {
        List<UsageEvent> events = new ArrayList<>();
        events.add(ev("flat", 0, "gpt-5", TokenCounts.of(1000, 500, 0, 0, 0)));
        events.add(ev("tiered", 1, "claude-sonnet-4-5",
                new TokenCounts(200_000, 10_000, 20_000, 30_000, java.util.OptionalLong.of(10_000), 2_000)));
        events.add(ev("base-1h", 2, "claude-sonnet-4-5",
                new TokenCounts(5_000, 1_000, 2_000, 4_000, java.util.OptionalLong.of(1_500), 0)));
        events.add(ev("alias", 3, "us.anthropic.claude-opus-4-6-20260115-v1:0", TokenCounts.of(800, 200, 0, 0, 0)));
        events.add(ev("miss", 4, "totally-unknown-model-9000", TokenCounts.of(100, 100, 0, 0, 0)));
        return events;
    }

    private static UsageEvent ev(String mid, int sec, String model, TokenCounts tokens) {
        return new UsageEvent("opencode", String.format("2026-08-01T00:0%d:00.000Z", sec),
                "s", mid, true, false, model, tokens,
                java.util.OptionalLong.empty(), java.util.OptionalDouble.empty(),
                java.util.Optional.empty(), java.util.Optional.empty());
    }

    private static void testPricingRoundtripMatchesGold(Path repoRoot) {
        String pinned = "2026-08-01T00:00:00.000Z";
        Object openrouter = readJson(repoRoot.resolve("golden/pricing/catalogs/openrouter.json"));
        Object litellm = readJson(repoRoot.resolve("golden/pricing/catalogs/litellm.json"));

        PricingOptions opts = PricingOptions.builder()
                .mode(PricingMode.CALCULATE)
                .sources(List.of())
                .catalog(Catalog.raw("openrouter", pinned, "openrouter", openrouter))
                .catalog(Catalog.raw("litellm", pinned, "litellm", litellm))
                .build();

        try (Pricing pricing = Skopli.createPricing(opts)) {
            List<CatalogInfo> infos = pricing.catalogs();
            assertEq(2, infos.size(), "two catalogs");
            assertEq("openrouter", infos.get(0).source(), "catalog[0] source");
            assertEq("litellm", infos.get(1).source(), "catalog[1] source");
            assertTrue(infos.get(0).models() > 0, "openrouter has models");

            List<PricedRollup> priced = pricing.priceEvents(pricingEvents(), RollupOptions.by(RollupBy.MODEL));

            // The committed gold (expected-priced-rollup.json) is produced by the
            // core's price_rollups (aggregate-then-cost); the C ABI exposes only
            // price_events (cost-per-event-then-sum), so the tiered-aggregate
            // group's usd differs by construction (1.800975 vs 1.84245 - a known
            // facade-sugar gap). Assert STRUCTURAL PARITY vs gold on the groups
            // where the two agree exactly (flat single-call hits + the miss),
            // plus group-count and key-set parity, matching the capi and Go
            // smoke tests.
            Object gold = readJson(repoRoot.resolve("golden/pricing/basic/expected-priced-rollup.json"));
            List<?> goldRollups = (List<?>) Wire.asObject(gold).get("rollups");
            assertEq(goldRollups.size(), priced.size(), "priced group count == gold");

            // Every gold key is present in the priced output.
            for (Object g : goldRollups) {
                String key = Wire.asString(Wire.asObject(g).get("key"));
                assertTrue(priced.stream().anyMatch(p -> p.rollup().key().equals(key)),
                        "priced output has gold group " + key);
            }

            // Flat single-call hit (gpt-5) matches gold pricing shape exactly.
            assertGoldPricing(goldRollups, priced, "gpt-5");
            // Aliased single-call hit (Bedrock -> anthropic key) matches gold exactly.
            assertGoldPricing(goldRollups, priced, "us.anthropic.claude-opus-4-6-20260115-v1:0");
            // Miss matches gold exactly.
            assertGoldPricing(goldRollups, priced, "totally-unknown-model-9000");

            // Sealed hit/miss discrimination via pattern matching.
            PricedRollup gpt5 = priced.stream().filter(p -> p.rollup().key().equals("gpt-5")).findFirst().orElseThrow();
            assertTrue(gpt5.pricing() instanceof PriceLookup.PriceHit, "gpt-5 is a hit");
            PricedRollup miss = priced.stream().filter(p -> p.rollup().key().equals("totally-unknown-model-9000")).findFirst().orElseThrow();
            assertTrue(miss.pricing() instanceof PriceLookup.PriceMiss, "unknown model is a miss");
        }
    }

    // -- priceRollups full gold equality ---------------------------------

    /** A hermetic pricing handle: explicit catalogs only, no built-in sources. */
    private static Pricing hermeticPricing(Path repoRoot) {
        String pinned = "2026-08-01T00:00:00.000Z";
        Object openrouter = readJson(repoRoot.resolve("golden/pricing/catalogs/openrouter.json"));
        Object litellm = readJson(repoRoot.resolve("golden/pricing/catalogs/litellm.json"));
        PricingOptions opts = PricingOptions.builder()
                .mode(PricingMode.CALCULATE)
                .sources(List.of())
                .catalog(Catalog.raw("openrouter", pinned, "openrouter", openrouter))
                .catalog(Catalog.raw("litellm", pinned, "litellm", litellm))
                .build();
        return Skopli.createPricing(opts);
    }

    private static void testPriceRollupsMatchesGold(Path repoRoot) {
        Object gold = readJson(repoRoot.resolve("golden/pricing/basic/expected-priced-rollup.json"));
        List<?> goldRollups = (List<?>) Wire.asObject(gold).get("rollups");

        // Input is a raw JSON copy of the gold rollups with only the `pricing`
        // field removed from each element - never decoded through facade types.
        List<Rollup> input = new ArrayList<>();
        for (Object g : goldRollups) {
            Map<String, Object> raw = new java.util.LinkedHashMap<>(Wire.asObject(g));
            raw.remove("pricing");
            input.add(Rollup.fromWire(raw));
        }

        try (Pricing pricing = hermeticPricing(repoRoot)) {
            List<PricedRollup> priced = pricing.priceRollups(input);
            assertEq(goldRollups.size(), priced.size(), "priceRollups group count == gold");
            // Full raw-JSON-tree equality: encode the actual returned values back
            // to a raw tree and compare the COMPLETE tree against the UNTOUCHED
            // gold rollups. The expected side is never decoded through facade
            // models; only key order and number formatting are canonicalized, and
            // identically on both sides.
            List<Object> goldTree = new ArrayList<>();
            for (Object g : goldRollups) {
                goldTree.add(canonical(g));
            }
            List<Object> actualTree = new ArrayList<>();
            for (PricedRollup p : priced) {
                actualTree.add(canonical(pricedRollupToComparable(p)));
            }
            assertEq(Json.write(goldTree), Json.write(actualTree), "priceRollups == gold (raw tree equality)");
        }
    }

    /** Canonicalize a parsed JSON value: recursively sort object keys and coerce
     * integral doubles to longs, so key order and number formatting differences
     * are erased. Applied identically to both compared trees. */
    private static Object canonical(Object value) {
        switch (value) {
            case Map<?, ?> m -> {
                java.util.TreeMap<String, Object> sorted = new java.util.TreeMap<>();
                for (Map.Entry<?, ?> e : m.entrySet()) {
                    sorted.put(String.valueOf(e.getKey()), canonical(e.getValue()));
                }
                return sorted;
            }
            case List<?> l -> {
                List<Object> out = new ArrayList<>(l.size());
                for (Object o : l) {
                    out.add(canonical(o));
                }
                return out;
            }
            case Double d -> {
                if (d == Math.floor(d) && !d.isInfinite()) {
                    return (long) (double) d;
                }
                return d;
            }
            default -> {
                return value;
            }
        }
    }

    private static void testLookupModelHitAndMiss(Path repoRoot) {
        try (Pricing pricing = hermeticPricing(repoRoot)) {
            PriceLookup hit = pricing.lookupModel("gpt-5");
            assertTrue(hit.priced(), "gpt-5 is a hit");
            assertTrue(hit instanceof PriceLookup.PriceHit, "gpt-5 lookup is a PriceHit");
            assertEq("litellm", ((PriceLookup.PriceHit) hit).source().orElse(null), "gpt-5 source == litellm");

            PriceLookup miss = pricing.lookupModel("totally-unknown-model-9000");
            assertTrue(!miss.priced(), "unknown model is a miss");
            assertTrue(miss instanceof PriceLookup.PriceMiss, "unknown lookup is a PriceMiss");
        }
    }

    // -- five-behavior lifecycle matrix (shared fixtures) -----------------

    private static Path lifecycleDir(Path repoRoot) {
        return repoRoot.resolve("golden/pricing/lifecycle");
    }

    /** A recording fetch: counts calls, then serves the payload (or throws). */
    private static final class RecordingFetch implements Fetcher {
        private final byte[] payload;
        private final boolean throws_;
        private int calls;

        RecordingFetch(byte[] payload, boolean throws_) {
            this.payload = payload;
            this.throws_ = throws_;
        }

        @Override
        public byte[] fetch(String url) throws Exception {
            calls++;
            if (throws_) {
                throw new Exception("network down");
            }
            return payload;
        }
    }

    /** The canonical raw source payload named by the fixture README (the bytes a
     * live fetch would return), read from source-&lt;name&gt;.json. */
    private static byte[] sourcePayload(Path repoRoot, String fixture) {
        Object payload = readJson(lifecycleDir(repoRoot).resolve(fixture));
        return Json.write(payload).getBytes(java.nio.charset.StandardCharsets.UTF_8);
    }

    private static void installCache(Path dir, String name, Path fixture) {
        try {
            Files.createDirectories(dir);
            Files.copy(fixture, dir.resolve(name));
        } catch (Exception e) {
            throw new RuntimeException("install cache: " + e, e);
        }
    }

    private static String currentFetchedAt(Path dir, String name) {
        Path path = dir.resolve(name);
        if (!Files.exists(path)) {
            return null;
        }
        Object file = readJson(path);
        return Wire.asString(Wire.asObject(file).get("fetchedAt"));
    }

    private static long parseIso(String iso) {
        return java.time.Instant.parse(iso).toEpochMilli();
    }

    /** Select the OpenRouter source factory named by {@code request.source}. The
     * five-behavior matrix pins {@code openrouter}; a mismatch fails loudly so a
     * fixture change to the source field cannot leave the suite green. */
    private static PricingSource sourceFactory(String source, String url) {
        return switch (source) {
            case "openrouter" -> Sources.openRouter(url);
            case "litellm" -> Sources.liteLlm(url);
            case "models-dev" -> Sources.modelsDev(url);
            default -> throw new IllegalStateException("unknown request source: " + source);
        };
    }

    private static void testLifecycleMatrix(Path repoRoot) {
        Object req = readJson(lifecycleDir(repoRoot).resolve("request.json"));
        Map<String, Object> r = Wire.asObject(req);
        String source = Wire.asString(r.get("source"));
        String cacheFileName = Wire.asString(r.get("cacheFileName"));
        long ttlMs = Wire.asLong(r.get("ttlMs"));
        long now = parseIso(Wire.asString(r.get("now")));
        long nowSecond = parseIso(Wire.asString(r.get("nowSecond")));
        // The request-level cache stamps: a behavior's expected fetchedAt must
        // equal the corresponding request stamp, so a fixture rename fails loudly
        // (mirroring the Swift LifecycleTests request-field assertions).
        String fetchedAtFresh = Wire.asString(r.get("fetchedAtFresh"));
        String fetchedAtStale = Wire.asString(r.get("fetchedAtStale"));
        String fetchedAtFetched = Wire.asString(r.get("fetchedAtFetched"));
        PricingMode mode = pricingMode(Wire.asString(Wire.asObject(r.get("options")).get("mode")));
        Rollup rollup = Rollup.fromWire(Wire.asObject(r.get("rollup")));

        // The canonical source payload named by the README (source-<name>.json),
        // not derived from the cache fixture, is the body a live fetch returns.
        byte[] sourceBody = sourcePayload(repoRoot, "source-" + source + ".json");
        byte[] emptyBody = sourcePayload(repoRoot, "source-" + source + "-empty.json");

        assertEq("openrouter", source, "request.source is openrouter");
        assertEq(PricingMode.CALCULATE, mode, "request.options.mode is calculate");

        runLifecycle(repoRoot, "cold-fetch", source, cacheFileName, ttlMs, now, mode, rollup, null, sourceBody, false, false, false, fetchedAtFetched);
        runLifecycle(repoRoot, "warm-cache", source, cacheFileName, ttlMs, now, mode, rollup, "cache-fresh.json", sourceBody, false, false, false, fetchedAtFresh);
        runLifecycle(repoRoot, "ttl-refresh", source, cacheFileName, ttlMs, now, mode, rollup, "cache-stale.json", sourceBody, false, true, false, fetchedAtFetched);
        runLifecycle(repoRoot, "offline", source, cacheFileName, ttlMs, now, mode, rollup, "cache-stale.json", sourceBody, true, false, false, fetchedAtStale);
        runLifecycle(repoRoot, "offline-no-cache", source, cacheFileName, ttlMs, now, mode, rollup, null, sourceBody, true, false, false, null);
        runLifecycle(repoRoot, "stale-fallback", source, cacheFileName, ttlMs, now, mode, rollup, "cache-stale.json", sourceBody, false, false, true, fetchedAtStale);
        // Fetch succeeds but returns a zero-price payload: treated like a fetch
        // failure - stale served, cache not overwritten.
        runLifecycle(repoRoot, "fetched-empty", source, cacheFileName, ttlMs, now, mode, rollup, "cache-stale.json", emptyBody, false, false, false, fetchedAtStale);
        // Fresh cache that parses to zero prices is unusable, same as no cache:
        // a fetch is issued and the cache is rewritten.
        runLifecycle(repoRoot, "cached-empty", source, cacheFileName, ttlMs, now, mode, rollup, "cache-fresh-empty.json", sourceBody, false, false, false, fetchedAtFetched);

        testPerSourceMatrix(repoRoot, ttlMs, now, mode, rollup);
        testLiveReload(repoRoot, cacheFileName, ttlMs, now, nowSecond, mode, rollup, sourceBody);
        testLiveReloadFailureRetries(repoRoot, ttlMs, now, nowSecond, mode, rollup, sourceBody);
    }

    private static void runLifecycle(Path repoRoot, String behavior, String source, String cacheFileName,
            long ttlMs, long now, PricingMode mode, Rollup rollup, String seedFixture, byte[] fetchBody,
            boolean offline, boolean refresh, boolean throws_, String requestStamp) {
        Object exp = readJson(lifecycleDir(repoRoot).resolve("expected/" + behavior + ".json"));
        Map<String, Object> e = Wire.asObject(exp);
        assertEq(behavior, Wire.asString(e.get("behavior")), behavior + ": fixture behavior label");
        boolean expFetched = Wire.asBool(e.get("fetched"));
        Object expFetchedAt = e.get("fetchedAt");
        boolean expPriced = Wire.asBool(e.get("priced"));
        double expUsd = Wire.asDouble(e.get("pricedUsd"));

        // The behavior's expected cache stamp must equal the request-level stamp
        // it is derived from (fetchedAtFresh/Stale/Fetched), so changing either
        // fixture fails loudly (mirroring the Swift request-field assertions).
        assertEq(requestStamp, expFetchedAt == null ? null : Wire.asString(expFetchedAt),
                behavior + ": expected fetchedAt == request stamp");

        Path dir;
        try {
            dir = Files.createTempDirectory("skopli-lifecycle-" + behavior);
        } catch (Exception ex) {
            throw new RuntimeException("temp dir: " + ex, ex);
        }
        if (seedFixture != null) {
            installCache(dir, cacheFileName, lifecycleDir(repoRoot).resolve(seedFixture));
        }
        RecordingFetch stub = new RecordingFetch(fetchBody, throws_);

        PricingOptions opts = PricingOptions.builder()
                .mode(mode)
                .sources(List.of(sourceFactory(source, "https://or.test/models")))
                .cacheDir(dir.toString())
                .ttlMs(ttlMs)
                .offline(offline)
                .refresh(refresh)
                .fetch(stub)
                .clock(Clock.fixed(now))
                .build();

        try (Pricing pricing = Skopli.createPricing(opts)) {
            List<PricedRollup> priced = pricing.priceRollups(List.of(rollup));
            assertEq(1, priced.size(), behavior + ": one priced rollup");
            PricedRollup got = priced.get(0);

            int wantCalls = expFetched ? 1 : 0;
            assertEq(wantCalls, stub.calls, behavior + ": fetch call count");
            assertEq(expPriced, got.pricing().priced(), behavior + ": priced flag");
            double usd = got.pricing().usd().orElse(0.0);
            assertTrue(Math.abs(usd - expUsd) < 1e-9, behavior + ": usd = " + usd + " want " + expUsd);

            String stamp = currentFetchedAt(dir, cacheFileName);
            if (expFetchedAt == null) {
                assertTrue(stamp == null, behavior + ": expected no cache file");
            } else {
                assertEq(Wire.asString(expFetchedAt), stamp, behavior + ": cache fetchedAt");
            }
        }
    }

    /** Table-driven per-source pass: each built-in source injected with its
     * canonical source-&lt;name&gt;.json body prices the shared rollup to 2.25,
     * stamps the exact provenance, and writes the exact cross-facade cache
     * filename. */
    private static void testPerSourceMatrix(Path repoRoot, long ttlMs, long now, PricingMode mode, Rollup rollup) {
        record Case(String source, String sourceFixture, String cacheFile) {}
        List<Case> cases = List.of(
                new Case("openrouter", "source-openrouter.json", "pricing-openrouter.json"),
                new Case("litellm", "source-litellm.json", "pricing-litellm.json"),
                new Case("models-dev", "source-modelsdev.json", "pricing-models-dev.json"));

        for (Case c : cases) {
            Path dir;
            try {
                dir = Files.createTempDirectory("skopli-persource-" + c.source());
            } catch (Exception ex) {
                throw new RuntimeException("temp dir: " + ex, ex);
            }
            byte[] body = sourcePayload(repoRoot, c.sourceFixture());
            RecordingFetch stub = new RecordingFetch(body, false);
            PricingOptions opts = PricingOptions.builder()
                    .mode(mode)
                    .sources(List.of(sourceFactory(c.source(), "https://" + c.source() + ".test/api")))
                    .cacheDir(dir.toString())
                    .ttlMs(ttlMs)
                    .fetch(stub)
                    .clock(Clock.fixed(now))
                    .build();
            try (Pricing pricing = Skopli.createPricing(opts)) {
                List<CatalogInfo> infos = pricing.catalogs();
                assertEq(1, infos.size(), c.source() + ": one catalog");
                assertEq(c.source(), infos.get(0).source(), c.source() + ": provenance");
                assertTrue(infos.get(0).models() > 0, c.source() + ": has models");

                List<PricedRollup> priced = pricing.priceRollups(List.of(rollup));
                double usd = priced.get(0).pricing().usd().orElse(0.0);
                assertTrue(Math.abs(usd - 2.25) < 1e-9, c.source() + ": priced 2.25 = " + usd);
                assertEq(1, stub.calls, c.source() + ": fetched once");

                String stamp = currentFetchedAt(dir, c.cacheFile());
                assertTrue(stamp != null, c.source() + ": wrote " + c.cacheFile());
            }
        }
    }

    /** The live-reload behavior: one long-lived instance, empty cacheDir. Query 1
     * at {@code now} fetches; the clock then advances past the TTL and query 2 on
     * the SAME instance reloads (the disk cache is stale), fetching again and
     * advancing the cache stamp. */
    private static void testLiveReload(Path repoRoot, String cacheFileName, long ttlMs, long now,
            long nowSecond, PricingMode mode, Rollup rollup, byte[] sourceBody) {
        Object exp = readJson(lifecycleDir(repoRoot).resolve("expected/live-reload.json"));
        Map<String, Object> e = Wire.asObject(exp);
        assertEq("live-reload", Wire.asString(e.get("behavior")), "live-reload: fixture behavior label");
        long fetchesTotal = Wire.asLong(e.get("fetchesTotal"));
        String fetchedAtFirst = Wire.asString(e.get("fetchedAtFirst"));
        String fetchedAtSecond = Wire.asString(e.get("fetchedAtSecond"));
        boolean expPriced = Wire.asBool(e.get("priced"));
        double expUsd = Wire.asDouble(e.get("pricedUsd"));

        Path dir;
        try {
            dir = Files.createTempDirectory("skopli-live-reload");
        } catch (Exception ex) {
            throw new RuntimeException("temp dir: " + ex, ex);
        }
        RecordingFetch stub = new RecordingFetch(sourceBody, false);
        MutableClock clock = new MutableClock(now);

        PricingOptions opts = PricingOptions.builder()
                .mode(mode)
                .sources(List.of(Sources.openRouter("https://or.test/models")))
                .cacheDir(dir.toString())
                .ttlMs(ttlMs)
                .fetch(stub)
                .clock(clock)
                .build();

        try (Pricing pricing = Skopli.createPricing(opts)) {
            List<PricedRollup> first = pricing.priceRollups(List.of(rollup));
            assertEq(expPriced, first.get(0).pricing().priced(), "live-reload: query 1 priced");
            assertTrue(Math.abs(first.get(0).pricing().usd().orElse(0.0) - expUsd) < 1e-9,
                    "live-reload: query 1 usd");
            assertEq(fetchedAtFirst, currentFetchedAt(dir, cacheFileName), "live-reload: first fetchedAt");

            // Advance the clock past the TTL; the same instance must reload.
            clock.set(nowSecond);
            List<PricedRollup> second = pricing.priceRollups(List.of(rollup));
            assertEq(expPriced, second.get(0).pricing().priced(), "live-reload: query 2 priced");
            assertTrue(Math.abs(second.get(0).pricing().usd().orElse(0.0) - expUsd) < 1e-9,
                    "live-reload: query 2 usd");
            assertEq(fetchedAtSecond, currentFetchedAt(dir, cacheFileName), "live-reload: second fetchedAt");
            assertEq((int) fetchesTotal, stub.calls, "live-reload: total fetches");
        }
    }

    /** A source whose Nth {@code load} (1-based) throws, driving a reload
     * failure; every other call serves a valid catalog built from the shared
     * openrouter payload (which prices the shared rollup to 2.25). */
    private static final class FlakySource implements PricingSource {
        private final Object payload;
        private final int throwOnCall;
        private int calls;

        FlakySource(Object payload, int throwOnCall) {
            this.payload = payload;
            this.throwOnCall = throwOnCall;
        }

        @Override
        public String name() {
            return "openrouter";
        }

        @Override
        public java.util.Optional<Catalog> load(SourceContext ctx) throws Exception {
            calls++;
            if (calls == throwOnCall) {
                throw new Exception("reload fetch failed");
            }
            return java.util.Optional.of(Catalog.raw("openrouter",
                    "2026-08-01T00:00:00.000Z", "openrouter", payload));
        }
    }

    /** Live-reload failure retry: load 1 succeeds; the first post-TTL reload's
     * source load throws so query 2 errors; query 3 retries the reload and
     * succeeds. Chosen TS-matching semantics: a rejected reload rejects THAT
     * query (index.ts:253-263 rejects the triggering query's promise and clears
     * the memoized load) and is not memoized, so the NEXT query retries. A prior
     * handle whose stamp is already expired is not resurrected as fresh; it stays
     * expired so the next query reloads rather than silently serving it. Load
     * counts prove the retry happened. */
    private static void testLiveReloadFailureRetries(Path repoRoot, long ttlMs, long now,
            long nowSecond, PricingMode mode, Rollup rollup, byte[] sourceBody) {
        Path dir;
        try {
            dir = Files.createTempDirectory("skopli-live-reload-retry");
        } catch (Exception ex) {
            throw new RuntimeException("temp dir: " + ex, ex);
        }
        Object payload = Json.parse(new String(sourceBody, java.nio.charset.StandardCharsets.UTF_8));
        FlakySource source = new FlakySource(payload, 2);
        MutableClock clock = new MutableClock(now);

        PricingOptions opts = PricingOptions.builder()
                .mode(mode)
                .sources(List.of(source))
                .cacheDir(dir.toString())
                .ttlMs(ttlMs)
                .clock(clock)
                .build();

        try (Pricing pricing = Skopli.createPricing(opts)) {
            // Query 1 at now: the first load (call 1) succeeds and prices.
            List<PricedRollup> first = pricing.priceRollups(List.of(rollup));
            assertTrue(Math.abs(first.get(0).pricing().usd().orElse(0.0) - 2.25) < 1e-9,
                    "live-reload-retry: query 1 usd");
            assertEq(1, source.calls, "live-reload-retry: query 1 loaded once");

            // Advance past the TTL. Query 2 triggers a reload whose load (call 2)
            // throws; the query itself errors and nothing is memoized.
            clock.set(nowSecond);
            boolean threw = false;
            try {
                pricing.priceRollups(List.of(rollup));
            } catch (CatalogException ex) {
                threw = true;
            }
            assertTrue(threw, "live-reload-retry: query 2 propagates the reload failure");
            assertEq(2, source.calls, "live-reload-retry: query 2 attempted the reload");

            // Query 3 (still past the TTL) retries the reload; the load (call 3)
            // succeeds. The retry proves the failed reload was not memoized.
            List<PricedRollup> third = pricing.priceRollups(List.of(rollup));
            assertTrue(Math.abs(third.get(0).pricing().usd().orElse(0.0) - 2.25) < 1e-9,
                    "live-reload-retry: query 3 usd after retry");
            assertEq(3, source.calls, "live-reload-retry: query 3 retried the reload");
        }
    }

    /** The shared golden constants contract (constants.json): the facade's own
     * source names, formats, URLs, priority order, TTL default, fetch timeout,
     * and cache filenames must equal the golden values so no facade's literals
     * drift. */
    private static void testLifecycleConstantsMatchGolden(Path repoRoot) {
        Object c = readJson(lifecycleDir(repoRoot).resolve("constants.json"));
        Map<String, Object> constants = Wire.asObject(c);

        Map<String, String> urlByName = Map.of(
                "openrouter", Sources.OPENROUTER_URL,
                "litellm", Sources.LITELLM_URL,
                "models-dev", Sources.MODELS_DEV_URL);

        // the default built-in sources in the reference priority order
        List<PricingSource> builtin = Sources.builtin();
        List<?> priority = (List<?>) constants.get("priorityOrder");
        List<?> sources = (List<?>) constants.get("sources");
        assertEq(priority.size(), builtin.size(), "constants: builtin source count == priorityOrder");

        Map<String, Sources.BuiltinSource> byName = new java.util.LinkedHashMap<>();
        for (int i = 0; i < builtin.size(); i++) {
            Sources.BuiltinSource src = (Sources.BuiltinSource) builtin.get(i);
            assertEq(Wire.asString(priority.get(i)), src.name(), "constants: priority[" + i + "]");
            byName.put(src.name(), src);
        }
        for (int i = 0; i < sources.size(); i++) {
            String specName = Wire.asString(Wire.asObject(sources.get(i)).get("name"));
            assertEq(Wire.asString(priority.get(i)), specName, "constants: sources[" + i + "].name == priority");
        }

        String pattern = Wire.asString(constants.get("cacheFileNamePattern"));
        for (Object node : sources) {
            Map<String, Object> spec = Wire.asObject(node);
            String name = Wire.asString(spec.get("name"));
            Sources.BuiltinSource src = byName.get(name);
            assertTrue(src != null, "constants: builtin source " + name + " present");
            assertEq(Wire.asString(spec.get("format")), src.format(), name + ": format");
            assertEq(Wire.asString(spec.get("url")), src.url(), name + ": url");
            assertEq(Wire.asString(spec.get("url")), urlByName.get(name), name + ": const URL");
            assertEq(pattern.replace("{name}", name), Wire.asString(spec.get("cacheFileName")),
                    name + ": cacheFileName pattern");
            // compare the production filename formula (Sources.cacheFileName), not
            // just the fixture pattern, so the real formula cannot drift unseen
            assertEq(Wire.asString(spec.get("cacheFileName")), Sources.cacheFileName(name),
                    name + ": cacheFileName formula");
        }

        assertEq(Wire.asLong(constants.get("defaultTtlMs")), PricingOptions.DEFAULT_TTL_MS,
                "constants: defaultTtlMs");
        // the facade keeps the fetch timeout as a Duration; the golden value is in ms
        assertEq(Wire.asLong(constants.get("fetchTimeoutMs")), Sources.FETCH_TIMEOUT.toMillis(),
                "constants: fetchTimeoutMs");
    }

    // -- probe contract vs shared gold ------------------------------------

    private static void testProbeConformance(Path repoRoot) {
        // the shared unusable-payload probe contract (probe/cases.json): the
        // facade's own probe (Ffi.probeModelCount, the same internal function its
        // lifecycle uses) must report usable/unusable exactly per case. An
        // invalid-JSON payloadRaw is fed as the raw payload value; the native
        // build parses it to zero.
        Object cases = readJson(repoRoot.resolve("golden/pricing/probe/cases.json"));
        for (Object node : (List<?>) cases) {
            Map<String, Object> kase = Wire.asObject(node);
            String name = Wire.asString(kase.get("name"));
            String format = Wire.asString(kase.get("format"));
            boolean usable = (boolean) kase.get("usable");
            Object payload = kase.containsKey("payloadRaw") ? kase.get("payloadRaw") : kase.get("payload");
            boolean got = com.skopli.internal.Ffi.probeModelCount("probe", format, payload) > 0;
            assertEq(usable, got, "probe: " + name);
        }
    }

    // -- cache timestamp boundary contract vs shared gold ----------------

    private static void testTimestampConformance(Path repoRoot) {
        // the shared cache fetchedAt boundary contract (timestamps/cases.json): a
        // stamp is read and written through the facade's own cache path
        // (com.skopli.internal.Cache.load / .store, the same the lifecycle uses).
        // A valid stamp resolves to its epoch, pinned to the millisecond by the
        // fresh/stale boundary at now == epochMs and now == epochMs + 1 with a
        // zero TTL; an invalid stamp yields null.
        Map<String, Object> cases = Wire.asObject(
                readJson(repoRoot.resolve("golden/pricing/timestamps/cases.json")));
        Path dir;
        try {
            dir = Files.createTempDirectory("skopli-timestamps");
        } catch (Exception ex) {
            throw new RuntimeException("temp dir: " + ex, ex);
        }
        String name = "pricing-openrouter.json";

        for (Object node : (List<?>) cases.get("read")) {
            Map<String, Object> kase = Wire.asObject(node);
            String label = Wire.asString(kase.get("name"));
            String stamp = Wire.asString(kase.get("stamp"));
            writeCacheFile(dir, name, stamp);
            Object epoch = kase.get("epochMs");
            if (epoch == null) {
                assertTrue(com.skopli.internal.Cache.load(dir.toString(), name, 0, 0) == null,
                        "timestamps: " + label);
                continue;
            }
            long epochMs = ((Number) epoch).longValue();
            com.skopli.internal.Cache.Cached fresh =
                    com.skopli.internal.Cache.load(dir.toString(), name, 0, epochMs);
            com.skopli.internal.Cache.Cached stale =
                    com.skopli.internal.Cache.load(dir.toString(), name, 0, epochMs + 1);
            assertTrue(fresh != null && !fresh.stale() && stale != null && stale.stale(),
                    "timestamps: " + label);
        }

        for (Object node : (List<?>) cases.get("write")) {
            long ms = ((Number) node).longValue();
            String stamp = com.skopli.internal.Cache.store(dir.toString(), name, Map.of(), ms);
            com.skopli.internal.Cache.Cached fresh =
                    com.skopli.internal.Cache.load(dir.toString(), name, 0, ms);
            com.skopli.internal.Cache.Cached stale =
                    com.skopli.internal.Cache.load(dir.toString(), name, 0, ms + 1);
            assertTrue(stamp.length() == 24 && stamp.endsWith("Z")
                            && fresh != null && !fresh.stale() && stale != null && stale.stale(),
                    "timestamps: write " + ms);
        }
    }

    private static void writeCacheFile(Path dir, String name, String stamp) {
        Map<String, Object> file = new java.util.LinkedHashMap<>();
        file.put("fetchedAt", stamp);
        file.put("payload", Map.of());
        try {
            Files.writeString(dir.resolve(name), Json.write(file));
        } catch (Exception ex) {
            throw new RuntimeException("write cache file: " + ex, ex);
        }
    }

    private static PricingMode pricingMode(String id) {
        for (PricingMode m : PricingMode.values()) {
            if (m.id().equals(id)) {
                return m;
            }
        }
        throw new IllegalStateException("unknown mode: " + id);
    }

    /** A clock whose value the test can advance to exercise TTL-expiry reloads. */
    private static final class MutableClock implements Clock {
        private volatile long millis;

        MutableClock(long millis) {
            this.millis = millis;
        }

        void set(long millis) {
            this.millis = millis;
        }

        @Override
        public long nowMillis() {
            return millis;
        }
    }

    /** A full PricedRollup as the comparable wire shape. */
    private static Map<String, Object> pricedRollupToComparable(PricedRollup p) {
        Map<String, Object> m = rollupToComparable(p.rollup());
        m.put("pricing", pricingToComparable(p.pricing()));
        return m;
    }

    private static void testPricingCloses(Path repoRoot) {
        String pinned = "2026-08-01T00:00:00.000Z";
        Object litellm = readJson(repoRoot.resolve("golden/pricing/catalogs/litellm.json"));
        PricingOptions opts = PricingOptions.builder()
                .sources(List.of())
                .catalog(Catalog.raw("litellm", pinned, "litellm", litellm))
                .build();
        Pricing pricing = Skopli.createPricing(opts);
        pricing.close();
        pricing.close(); // idempotent
        boolean threw = false;
        try {
            pricing.priceEvents(pricingEvents(), RollupOptions.by(RollupBy.MODEL));
        } catch (IllegalStateException e) {
            threw = true;
        }
        assertTrue(threw, "use-after-close throws IllegalStateException");
    }

    private static void testForcedCatalogError() {
        // A catalog with an unknown format forces a Catalog status from the core.
        PricingOptions opts = PricingOptions.builder()
                .sources(List.of())
                .catalog(Catalog.raw("bogus", null, "not-a-real-format", Map.of()))
                .build();
        boolean threw = false;
        try {
            Skopli.createPricing(opts);
        } catch (CatalogException e) {
            threw = true;
            assertTrue(e instanceof SkopliException, "CatalogException is an SkopliException");
        }
        assertTrue(threw, "unknown catalog format throws CatalogException");
    }

    /** Assert the priced group {@code key}'s pricing shape equals the gold's. */
    private static void assertGoldPricing(List<?> goldRollups, List<PricedRollup> priced, String key) {
        Map<String, Object> goldPricing = null;
        for (Object g : goldRollups) {
            Map<String, Object> group = Wire.asObject(g);
            if (Wire.asString(group.get("key")).equals(key)) {
                goldPricing = Wire.asObject(group.get("pricing"));
                break;
            }
        }
        if (goldPricing == null) {
            fail("gold has no group " + key);
        }
        PricedRollup actual = priced.stream()
                .filter(p -> p.rollup().key().equals(key))
                .findFirst()
                .orElseThrow(() -> new AssertionError("missing priced group " + key));
        Map<String, Object> actualPricing = pricingToComparable(actual.pricing());
        // The ABI's price_events hit carries the full "price" object; the gold
        // (price_rollups shape) omits it. Assert every gold field is present and
        // equal in the ABI output (superset parity) - the semantic fields
        // (priced/model/key/source/fetchedAt/usd/attempted) must match exactly.
        for (Map.Entry<String, Object> e : goldPricing.entrySet()) {
            assertEq(Json.write(e.getValue()), Json.write(actualPricing.get(e.getKey())),
                    "pricing." + e.getKey() + " for group " + key);
        }
    }

    // -- comparison helpers -----------------------------------------------

    /** A Rollup as a comparable wire map (matching the gold rollup grammar). */
    private static Map<String, Object> rollupToComparable(Rollup r) {
        Map<String, Object> m = new java.util.LinkedHashMap<>();
        m.put("key", r.key());
        m.put("tokens", tokensComparable(r.tokens()));
        m.put("events", r.events());
        m.put("turns", r.turns());
        m.put("calls", r.calls());
        r.costUsd().ifPresent(v -> m.put("costUsd", v));
        return m;
    }

    private static Map<String, Object> tokensComparable(TokenCounts t) {
        Map<String, Object> m = new java.util.LinkedHashMap<>();
        m.put("input", t.input());
        m.put("output", t.output());
        m.put("cacheRead", t.cacheRead());
        m.put("cacheWrite", t.cacheWrite());
        t.cacheWrite1h().ifPresent(v -> m.put("cacheWrite1h", v));
        m.put("reasoning", t.reasoning());
        return m;
    }

    /** Normalize a gold rollup object to the same key order/number types the
     * facade produces (drop schema-only fields, keep the same set). */
    private static Map<String, Object> normalizeRollup(Map<String, Object> gold) {
        Map<String, Object> m = new java.util.LinkedHashMap<>();
        m.put("key", Wire.asString(gold.get("key")));
        Map<String, Object> t = Wire.asObject(gold.get("tokens"));
        Map<String, Object> tokens = new java.util.LinkedHashMap<>();
        tokens.put("input", Wire.asLong(t.get("input")));
        tokens.put("output", Wire.asLong(t.get("output")));
        tokens.put("cacheRead", Wire.asLong(t.get("cacheRead")));
        tokens.put("cacheWrite", Wire.asLong(t.get("cacheWrite")));
        if (t.get("cacheWrite1h") != null) {
            tokens.put("cacheWrite1h", Wire.asLong(t.get("cacheWrite1h")));
        }
        tokens.put("reasoning", Wire.asLong(t.get("reasoning")));
        m.put("tokens", tokens);
        m.put("events", Wire.asLong(gold.get("events")));
        m.put("turns", Wire.asLong(gold.get("turns")));
        m.put("calls", Wire.asLong(gold.get("calls")));
        if (gold.get("costUsd") != null) {
            m.put("costUsd", Wire.asDouble(gold.get("costUsd")));
        }
        return m;
    }

    /** A PriceLookup as a comparable wire map matching the gold pricing shape. */
    private static Map<String, Object> pricingToComparable(PriceLookup lookup) {
        Map<String, Object> m = new java.util.LinkedHashMap<>();
        switch (lookup) {
            case PriceLookup.PriceHit hit -> {
                m.put("priced", true);
                m.put("model", hit.model());
                hit.key().ifPresent(v -> m.put("key", v));
                hit.price().ifPresent(p -> m.put("price", p.toWire()));
                hit.source().ifPresent(v -> m.put("source", v));
                m.put("fetchedAt", hit.fetchedAt().orElse(null));
                hit.usd().ifPresent(v -> m.put("usd", v));
                if (hit.tieredAggregate()) {
                    m.put("tieredAggregate", true);
                }
            }
            case PriceLookup.PriceMiss miss -> {
                m.put("priced", false);
                m.put("model", miss.model());
                m.put("attempted", miss.attempted());
                miss.reason().ifPresent(v -> {
                    m.put("reason", v);
                    m.put("key", miss.key().orElse(null));
                });
                miss.usd().ifPresent(v -> m.put("usd", v));
            }
        }
        return m;
    }

    private static Object readJson(Path path) {
        try {
            return Json.parse(Files.readString(path));
        } catch (Exception e) {
            throw new RuntimeException("reading " + path + ": " + e, e);
        }
    }

    private static Path discoverRepoRoot() {
        Path p = Path.of("").toAbsolutePath();
        while (p != null) {
            if (Files.isDirectory(p.resolve("golden")) && Files.isDirectory(p.resolve("crates"))) {
                return p;
            }
            p = p.getParent();
        }
        throw new IllegalStateException("could not find repo root (no golden/ + crates/ ancestor)");
    }

    // -- assertions -------------------------------------------------------

    private static void assertTrue(boolean cond, String what) {
        checks++;
        if (!cond) {
            fail(what);
        }
        System.out.println("  ok: " + what);
    }

    private static void assertEq(Object expected, Object actual, String what) {
        checks++;
        if (!java.util.Objects.equals(expected, actual)) {
            fail(what + "\n    expected: " + expected + "\n    actual:   " + actual);
        }
        System.out.println("  ok: " + what);
    }

    private static void fail(String what) {
        System.err.println("FAILED: " + what);
        throw new AssertionError(what);
    }
}
