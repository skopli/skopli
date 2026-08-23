"""Python smoke test: read -> rollup -> price against a golden case.

Exercises the typed facade end to end and compares the results structurally
against the committed gold envelopes (``golden/claude/basic``): a native abi3
wheel, installed, driving the whole pipeline with parity against the shared
conformance fixtures.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

import skopli
from skopli import (
    Detection,
    ModelPrice,
    PriceHit,
    PriceMiss,
    ReadUsageResult,
    Rollup,
    TokenCounts,
)

REPO_ROOT = Path(__file__).resolve().parents[3]
CASE = REPO_ROOT / "golden" / "claude" / "basic"
LIFECYCLE = REPO_ROOT / "golden" / "pricing" / "lifecycle"
PRICING_BASIC = REPO_ROOT / "golden" / "pricing" / "basic"
CATALOGS = REPO_ROOT / "golden" / "pricing" / "catalogs"
REGISTRY_IDS = REPO_ROOT / "golden" / "registry" / "ids.json"


def test_harness_enum_covers_every_registry_id() -> None:
    # The shared registry-id fixture is the canonical list of harness ids the
    # core registers; the published Harness enum must cover every one so a reader
    # added to the core cannot silently fall out of the typed surface.
    ids = json.loads(REGISTRY_IDS.read_text(encoding="utf-8"))["ids"]
    values = {member.value for member in skopli.Harness}
    missing = [harness for harness in ids if harness not in values]
    assert not missing, f"Harness enum missing registry ids: {missing}"
    extra = sorted(values - set(ids))
    assert not extra, f"Harness enum has ids the registry does not: {extra}"


def _load(name: str) -> dict:
    return json.loads((CASE / name).read_text(encoding="utf-8"))


def _manifest() -> dict:
    return _load("input-manifest.json")


def _config_dir() -> str:
    # The manifest's env maps CLAUDE_CONFIG_DIR to "." relative to the input dir.
    return str(CASE / "input")


def _read() -> ReadUsageResult:
    manifest = _manifest()
    env = {"CLAUDE_CONFIG_DIR": _config_dir()}
    return skopli.read_usage(
        home=_config_dir(),
        env=env,
        harnesses=manifest["options"]["harnesses"],
        tz=manifest["options"].get("tz"),
    )


def test_case_fixture_exists() -> None:
    assert CASE.is_dir(), f"golden case missing: {CASE}"


def test_read_usage_matches_gold_events() -> None:
    expected = _load("expected-events.json")["events"]
    result = _read()

    assert isinstance(result, ReadUsageResult)
    assert len(result.events) == len(expected)

    # Structural parity: re-serialize each typed event to the wire shape and
    # compare against the gold events (order-preserving, as the reader emits).
    got_wire = [e.to_wire() for e in result.events]
    assert got_wire == expected

    # Typed access works.
    first = result.events[0]
    assert first.harness == "claude"
    assert isinstance(first.tokens, TokenCounts)
    assert first.tokens.input == expected[0]["tokens"]["input"]


def test_detect_harnesses_finds_claude() -> None:
    detection = skopli.detect_harnesses(
        home=_config_dir(),
        env={"CLAUDE_CONFIG_DIR": _config_dir()},
    )
    assert isinstance(detection, Detection)
    assert "claude" in detection.supported


@pytest.mark.parametrize("by", ["model", "day", "harness"])
def test_rollup_matches_gold(by: str) -> None:
    expected = _load("expected-rollup.json")["by"][by]
    events = _read().events
    rollups = skopli.rollup(events, by, tz="UTC")

    assert all(isinstance(r, Rollup) for r in rollups)

    # Strip the derived costUsd from both sides: it is a pricing-layer field the
    # bare rollup should not add, but the gold rollup includes it where the TS
    # exporter priced. Compare token/event/turn/call structure by key.
    got = {r.key: r.to_wire() for r in rollups}
    exp = {r["key"]: r for r in expected}
    assert set(got) == set(exp)
    for key in exp:
        g = {k: v for k, v in got[key].items() if k != "costUsd"}
        e = {k: v for k, v in exp[key].items() if k != "costUsd"}
        assert g == e, f"rollup {by}/{key} mismatch"


def test_cost_usd_flat_price() -> None:
    tokens = TokenCounts(input=1000, output=500)
    price = ModelPrice(input=1.25, output=10.0)
    # (1000/1e6)*1.25 + (500/1e6)*10 = 0.00125 + 0.005 = 0.00625
    assert skopli.cost_usd(tokens, price) == pytest.approx(0.00625)


def test_price_rollups_hit_and_miss() -> None:
    events = _read().events
    rollups = skopli.rollup(events, "model", tz="UTC")

    pricing = skopli.create_pricing(
        overrides=[
            {"model": "claude-sonnet-4-5-20250929", "input": 3.0, "output": 15.0},
        ],
        sources=[],
    )
    priced = pricing.price_rollups(rollups)
    assert priced

    by_key = {p.rollup.key: p for p in priced}

    # The overridden model is a hit with a computed usd.
    sonnet = by_key["claude-sonnet-4-5-20250929"]
    assert isinstance(sonnet.pricing, PriceHit)
    assert sonnet.pricing.source == "override"
    assert sonnet.usd is not None and sonnet.usd > 0

    # The unknown model is a miss.
    mystery = by_key["mystery-model-x"]
    assert isinstance(mystery.pricing, PriceMiss)
    assert not mystery.pricing.priced


def test_price_events_grouped() -> None:
    events = _read().events
    pricing = skopli.create_pricing(
        overrides=[
            {"model": "claude-sonnet-4-5-20250929", "input": 3.0, "output": 15.0},
        ],
        sources=[],
    )
    groups = pricing.price_events(events, "day", tz="UTC")
    assert groups
    keys = {g.rollup.key for g in groups}
    assert "2026-08-01" in keys


def test_lookup_model_union_matches() -> None:
    pricing = skopli.create_pricing(
        overrides=[{"model": "known", "input": 1.0, "output": 2.0}],
        sources=[],
    )
    hit = pricing.lookup_model("known")
    match hit:
        case PriceHit(source=source):
            assert source == "override"
        case _:
            pytest.fail("expected a PriceHit")

    miss = pricing.lookup_model("unknown-xyz")
    assert isinstance(miss, PriceMiss)


def test_invalid_argument_raises_value_error() -> None:
    with pytest.raises(skopli.InvalidArgumentError):
        skopli.rollup([], "not-a-dimension")
    # InvalidArgumentError is also a ValueError (idiom parity).
    with pytest.raises(ValueError):
        skopli.rollup([], "not-a-dimension")


def test_default_cache_dir_nonempty() -> None:
    assert skopli.default_cache_dir()


def _lifecycle(name: str) -> dict:
    return json.loads((LIFECYCLE / name).read_text(encoding="utf-8"))


def _iso_ms(iso: str) -> float:
    from datetime import datetime

    return datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp() * 1000


class _RecordingFetch:
    def __init__(self, payload, throws: bool = False) -> None:
        self.payload = payload
        self.throws = throws
        self.calls = 0

    def __call__(self, url: str):
        self.calls += 1
        if self.throws:
            raise RuntimeError("network down")
        return self.payload


def _builtin_source(request):
    # Select the built-in source named by request.source and load its canonical
    # source-<name>.json as the successful fetch body (never derive it from a
    # cache fixture payload). This is what the README pins as the fetch response.
    name = request["source"]
    url = f"https://{name}.test/api"
    payload = _lifecycle(f"source-{name}.json")
    return skopli.BuiltinSource(name, name, url), payload


def _read_expected(behavior: str) -> dict:
    expected = _lifecycle(f"expected/{behavior}.json")
    assert expected["behavior"] == behavior
    return expected


def _run_behavior(tmp_path, expected, *, install=None, offline=False, refresh=False, throws=False):
    # Drive one lifecycle behavior against the committed fixtures and consume
    # every key of expected/<behavior>.json: behavior (via _read_expected),
    # exact fetch count, priced, pricedUsd, and the exact fetchedAt (or its
    # absence). The source and fetch body come from request.source, never a
    # hard-coded name or a cache-derived payload.
    request = _lifecycle("request.json")
    source, source_payload = _builtin_source(request)
    cache_file = tmp_path / request["cacheFileName"]
    if install is not None:
        cache_file.write_text(json.dumps(_lifecycle(install)), encoding="utf-8")
    now_ms = _iso_ms(request["now"])
    stub = _RecordingFetch(source_payload, throws=throws)
    pricing = skopli.create_pricing(
        sources=[source],
        cache_dir=str(tmp_path),
        ttl_ms=request["ttlMs"],
        offline=offline,
        refresh=refresh,
        fetch=stub,
        _clock=lambda: now_ms,
        **request["options"],
    )
    rollup = skopli.Rollup.from_wire(request["rollup"])
    priced = pricing.price_rollups([rollup])
    pr = priced[0]

    assert stub.calls == int(expected["fetched"])
    if expected["priced"]:
        assert isinstance(pr.pricing, PriceHit)
        assert pr.usd == pytest.approx(expected["pricedUsd"], abs=1e-10)
    else:
        assert isinstance(pr.pricing, PriceMiss)
        assert (pr.usd or 0) == expected["pricedUsd"]

    if expected["fetchedAt"] is None:
        assert not cache_file.exists()
    else:
        body = json.loads(cache_file.read_text(encoding="utf-8"))
        assert body["fetchedAt"] == expected["fetchedAt"]


def test_lifecycle_cold_fetch(tmp_path) -> None:
    _run_behavior(tmp_path, _read_expected("cold-fetch"))


def test_cache_interop_written_file_parses_fresh(tmp_path) -> None:
    from skopli import _load_cached, _store_cached

    request = _lifecycle("request.json")
    payload = _lifecycle("cache-fresh.json")["payload"]
    now_ms = _iso_ms(request["now"])
    fetched_at = _store_cached(str(tmp_path), request["cacheFileName"], payload, now_ms)
    assert fetched_at == request["fetchedAtFetched"]

    body = json.loads((tmp_path / request["cacheFileName"]).read_text(encoding="utf-8"))
    assert body == {"fetchedAt": request["fetchedAtFetched"], "payload": payload}

    cached = _load_cached(str(tmp_path), request["cacheFileName"], request["ttlMs"], now_ms)
    assert cached is not None
    assert cached["stale"] is False
    assert cached["fetchedAt"] == request["fetchedAtFetched"]


def test_lifecycle_warm_cache(tmp_path) -> None:
    _run_behavior(tmp_path, _read_expected("warm-cache"), install="cache-fresh.json")


def test_lifecycle_ttl_refresh(tmp_path) -> None:
    _run_behavior(
        tmp_path, _read_expected("ttl-refresh"), install="cache-stale.json", refresh=True
    )


def test_lifecycle_offline(tmp_path) -> None:
    _run_behavior(tmp_path, _read_expected("offline"), install="cache-stale.json", offline=True)


def test_lifecycle_offline_no_cache(tmp_path) -> None:
    _run_behavior(tmp_path, _read_expected("offline-no-cache"), offline=True)


def test_lifecycle_stale_fallback(tmp_path) -> None:
    _run_behavior(
        tmp_path, _read_expected("stale-fallback"), install="cache-stale.json", throws=True
    )


def test_lifecycle_fetched_empty(tmp_path) -> None:
    # a stale cache is present; the fetch succeeds but returns a payload that
    # parses to zero prices. The unusable payload is discarded like a failure:
    # the stale catalog is served, the cache file is NOT overwritten.
    expected = _read_expected("fetched-empty")
    request = _lifecycle("request.json")
    source, _ = _builtin_source(request)
    empty_payload = _lifecycle("source-openrouter-empty.json")
    cache_file = tmp_path / request["cacheFileName"]
    cache_file.write_text(json.dumps(_lifecycle("cache-stale.json")), encoding="utf-8")
    now_ms = _iso_ms(request["now"])
    stub = _RecordingFetch(empty_payload)
    pricing = skopli.create_pricing(
        sources=[source],
        cache_dir=str(tmp_path),
        ttl_ms=request["ttlMs"],
        fetch=stub,
        _clock=lambda: now_ms,
        **request["options"],
    )
    priced = pricing.price_rollups([skopli.Rollup.from_wire(request["rollup"])])
    assert stub.calls == int(expected["fetched"])
    if expected["priced"]:
        assert isinstance(priced[0].pricing, PriceHit)
    else:
        assert isinstance(priced[0].pricing, PriceMiss)
    assert priced[0].usd == pytest.approx(expected["pricedUsd"], abs=1e-10)
    body = json.loads(cache_file.read_text(encoding="utf-8"))
    assert body["fetchedAt"] == expected["fetchedAt"]
    assert body["fetchedAt"] == request["fetchedAtStale"]


def test_lifecycle_cached_empty(tmp_path) -> None:
    # a fresh-aged cache whose payload parses to zero prices is unusable, same
    # as no cache: a fetch is issued, returns the canonical payload, the cache
    # is rewritten with fetchedAt = now, and the rollup prices to gold.
    expected = _read_expected("cached-empty")
    request = _lifecycle("request.json")
    source, good_payload = _builtin_source(request)
    cache_file = tmp_path / request["cacheFileName"]
    cache_file.write_text(json.dumps(_lifecycle("cache-fresh-empty.json")), encoding="utf-8")
    now_ms = _iso_ms(request["now"])
    stub = _RecordingFetch(good_payload)
    pricing = skopli.create_pricing(
        sources=[source],
        cache_dir=str(tmp_path),
        ttl_ms=request["ttlMs"],
        fetch=stub,
        _clock=lambda: now_ms,
        **request["options"],
    )
    priced = pricing.price_rollups([skopli.Rollup.from_wire(request["rollup"])])
    assert stub.calls == int(expected["fetched"])
    if expected["priced"]:
        assert isinstance(priced[0].pricing, PriceHit)
    else:
        assert isinstance(priced[0].pricing, PriceMiss)
    assert priced[0].usd == pytest.approx(expected["pricedUsd"], abs=1e-10)
    body = json.loads(cache_file.read_text(encoding="utf-8"))
    assert body["fetchedAt"] == expected["fetchedAt"]
    assert body["fetchedAt"] == request["fetchedAtFetched"]


def _read_live_reload_expected() -> dict:
    # live-reload pins a different key set from the single-query behaviors, so it
    # gets its own consuming helper: behavior, fetchesTotal, fetchedAtFirst,
    # fetchedAtSecond, priced, pricedUsd are all read and asserted below.
    expected = _lifecycle("expected/live-reload.json")
    assert expected["behavior"] == "live-reload"
    return expected


def test_lifecycle_live_reload(tmp_path) -> None:
    # one long-lived instance, empty cacheDir. Query 1 at `now` fetches and
    # prices; the clock then advances past the TTL window, so query 2 on the
    # SAME instance consults sources again (the disk cache is now stale) and a
    # second fetch is issued. Total fetches = fetchesTotal (2). Every key of
    # expected/live-reload.json is consumed across both queries.
    expected = _read_live_reload_expected()
    request = _lifecycle("request.json")
    source, good_payload = _builtin_source(request)
    cache_file = tmp_path / request["cacheFileName"]
    clock = {"ms": _iso_ms(request["now"])}
    stub = _RecordingFetch(good_payload)
    pricing = skopli.create_pricing(
        sources=[source],
        cache_dir=str(tmp_path),
        ttl_ms=request["ttlMs"],
        fetch=stub,
        _clock=lambda: clock["ms"],
        **request["options"],
    )
    first = pricing.price_rollups([skopli.Rollup.from_wire(request["rollup"])])
    if expected["priced"]:
        assert isinstance(first[0].pricing, PriceHit)
    else:
        assert isinstance(first[0].pricing, PriceMiss)
    assert first[0].usd == pytest.approx(expected["pricedUsd"], abs=1e-10)
    assert json.loads(cache_file.read_text(encoding="utf-8"))["fetchedAt"] == expected["fetchedAtFirst"]

    clock["ms"] = _iso_ms(request["nowSecond"])
    second = pricing.price_rollups([skopli.Rollup.from_wire(request["rollup"])])
    if expected["priced"]:
        assert isinstance(second[0].pricing, PriceHit)
    else:
        assert isinstance(second[0].pricing, PriceMiss)
    assert second[0].usd == pytest.approx(expected["pricedUsd"], abs=1e-10)
    assert stub.calls == expected["fetchesTotal"]
    assert json.loads(cache_file.read_text(encoding="utf-8"))["fetchedAt"] == expected["fetchedAtSecond"]


@pytest.mark.parametrize(
    "file,name,fmt,cache_file",
    [
        ("source-openrouter.json", "openrouter", "openrouter", "pricing-openrouter.json"),
        ("source-litellm.json", "litellm", "litellm", "pricing-litellm.json"),
        ("source-modelsdev.json", "models-dev", "modelsdev", "pricing-models-dev.json"),
    ],
)
def test_lifecycle_source_fixtures_price_to_gold(
    tmp_path, file: str, name: str, fmt: str, cache_file: str
) -> None:
    # Each built-in source has a distinct public name and a parser format: the
    # public identity for models.dev is `models-dev` (not `modelsdev`), so its
    # shared cache filename is pricing-models-dev.json. Assert the fetch fires
    # exactly once, the catalog provenance reports the exact source name and a
    # fetchedAt of now, the exact cache file lands on disk, and the price is 2.25.
    request = _lifecycle("request.json")
    payload = _lifecycle(file)
    stub = _RecordingFetch(payload)
    now_ms = _iso_ms(request["now"])
    pricing = skopli.create_pricing(
        sources=[skopli.BuiltinSource(name, fmt, f"https://{name}.test/api")],
        cache_dir=str(tmp_path),
        ttl_ms=request["ttlMs"],
        fetch=stub,
        _clock=lambda: now_ms,
        **request["options"],
    )
    priced = pricing.price_rollups([skopli.Rollup.from_wire(request["rollup"])])
    assert stub.calls == 1
    assert isinstance(priced[0].pricing, PriceHit)
    assert priced[0].usd == pytest.approx(2.25, abs=1e-10)

    infos = {c.source: c for c in pricing.catalogs()}
    assert name in infos
    assert infos[name].fetched_at == request["fetchedAtFetched"]

    assert (tmp_path / cache_file).exists()


def test_lifecycle_constants_match_golden() -> None:
    # the shared golden constants contract (constants.json): the facade's own
    # source names, formats, URLs, priority order, TTL default, fetch timeout,
    # and cache filenames must equal the golden values so no facade's literals
    # drift.
    constants = _lifecycle("constants.json")

    url_by_name = {
        "openrouter": skopli.OPENROUTER_URL,
        "litellm": skopli.LITELLM_URL,
        "models-dev": skopli.MODELS_DEV_URL,
    }

    # the default built-in sources in the reference priority order
    builtin = skopli._builtin_sources()
    assert [s.name for s in builtin] == constants["priorityOrder"]
    assert constants["priorityOrder"] == [s["name"] for s in constants["sources"]]

    by_name = {s.name: s for s in builtin}
    for spec in constants["sources"]:
        name = spec["name"]
        source = by_name[name]
        assert source.format == spec["format"], name
        assert source.url == spec["url"], name
        assert url_by_name[name] == spec["url"], name
        # compare the production filename formula (_cache_file_name), not just
        # the fixture pattern, so the real formula cannot drift unseen
        assert skopli._cache_file_name(name) == spec["cacheFileName"], name
        assert spec["cacheFileName"] == constants["cacheFileNamePattern"].replace(
            "{name}", name
        ), name

    assert skopli.DEFAULT_TTL_MS == constants["defaultTtlMs"]
    # the facade keeps the fetch timeout in seconds; the golden value is in ms
    assert int(skopli.FETCH_TIMEOUT_S * 1000) == constants["fetchTimeoutMs"]


PROBE = REPO_ROOT / "golden" / "pricing" / "probe"


@pytest.mark.parametrize(
    "case",
    json.loads((PROBE / "cases.json").read_text(encoding="utf-8")),
    ids=lambda c: c["name"],
)
def test_probe_conformance(case) -> None:
    # the shared unusable-payload probe contract (probe/cases.json): the facade's
    # own probe (_probe_prices, the same internal function its lifecycle uses)
    # must report usable/unusable exactly per case. An invalid-JSON payloadRaw is
    # fed as the raw payload value; the native build parses it to zero.
    payload = case["payloadRaw"] if "payloadRaw" in case else case["payload"]
    entry = {
        "source": "probe",
        "fetchedAt": None,
        "format": case["format"],
        "payload": payload,
    }
    assert (skopli._probe_prices(entry) > 0) == case["usable"], case["name"]


ROLLUP_TZ = REPO_ROOT / "golden" / "rollup-tz"


@pytest.mark.parametrize(
    "case",
    json.loads((ROLLUP_TZ / "cases.json").read_text(encoding="utf-8")),
    ids=lambda c: c["name"],
)
def test_rollup_tz_conformance(case) -> None:
    # the shared timezone day-bucketing contract (rollup-tz/cases.json): one
    # synthetic event per timestamp, rolled up by day in the case's zone, must
    # produce exactly the expected buckets. This reaches the same native core the
    # Rust and other-language suites exercise.
    events = [
        {
            "harness": "h",
            "timestamp": ts,
            "sessionId": "s",
            "messageId": "m",
            "turn": False,
            "subagent": False,
            "model": "m",
            "tokens": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "reasoning": 0},
        }
        for ts in case["timestamps"]
    ]
    rollups = skopli.rollup(events, "day", tz=case["tz"])
    got = [{"key": r.key, "events": r.events} for r in rollups]
    assert got == case["expected"], case["name"]


ROLLUP_BLOCK = REPO_ROOT / "golden" / "rollup-block"


@pytest.mark.parametrize(
    "case",
    json.loads((ROLLUP_BLOCK / "cases.json").read_text(encoding="utf-8")),
    ids=lambda c: c["name"],
)
def test_rollup_block_conformance(case) -> None:
    # the shared billing-block windowing contract (rollup-block/cases.json): one
    # synthetic event per timestamp, rolled up into blocks of the case's width in
    # the case's zone, must produce exactly the expected blocks. This reaches the
    # same native core the Rust and other-language suites exercise.
    events = [
        {
            "harness": "h",
            "timestamp": ts,
            "sessionId": "s",
            "messageId": "m",
            "turn": False,
            "subagent": False,
            "model": "m",
            "tokens": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "reasoning": 0},
        }
        for ts in case["timestamps"]
    ]
    rollups = skopli.rollup(events, "block", tz=case["tz"], block_ms=case["blockMs"])
    got = [{"key": r.key, "events": r.events} for r in rollups]
    assert got == case["expected"], case["name"]


def test_rollup_block_omitted_width_uses_default() -> None:
    # omitting block_ms must apply the five-hour default across the wire: two
    # events 3h41m apart join one block anchored to 09:00, and an event past
    # five hours opens a new block. Drives the production rollup with no width so
    # the facade's absent-optional serialization is exercised, not the fixture's
    # explicit value.
    def events(*timestamps):
        return [
            {
                "harness": "h",
                "timestamp": ts,
                "sessionId": "s",
                "messageId": "m",
                "turn": False,
                "subagent": False,
                "model": "m",
                "tokens": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "reasoning": 0},
            }
            for ts in timestamps
        ]

    joined = skopli.rollup(
        events("2026-01-01T09:17:00.000Z", "2026-01-01T13:00:00.000Z"), "block", tz="UTC"
    )
    assert [{"key": r.key, "events": r.events} for r in joined] == [
        {"key": "2026-01-01T09:00:00.000Z", "events": 2}
    ]

    split = skopli.rollup(
        events("2026-01-01T09:00:00.000Z", "2026-01-01T14:30:00.000Z"), "block", tz="UTC"
    )
    assert [{"key": r.key, "events": r.events} for r in split] == [
        {"key": "2026-01-01T09:00:00.000Z", "events": 1},
        {"key": "2026-01-01T14:00:00.000Z", "events": 1},
    ]


TIMESTAMPS = REPO_ROOT / "golden" / "pricing" / "timestamps"
_TS_CASES = json.loads((TIMESTAMPS / "cases.json").read_text(encoding="utf-8"))


@pytest.mark.parametrize("case", _TS_CASES["read"], ids=lambda c: c["name"])
def test_timestamp_read_conformance(case, tmp_path) -> None:
    # the shared cache fetchedAt boundary contract (timestamps/cases.json): a
    # stamp is read through the facade's own cache-read path (_load_cached, the
    # same function its lifecycle uses). An invalid stamp yields None; a valid
    # stamp resolves to its epoch, pinned to the millisecond by the fresh/stale
    # boundary at now == epochMs and now == epochMs + 1 with a zero TTL.
    from skopli import _load_cached

    name = "pricing-openrouter.json"
    (tmp_path / name).write_text(
        json.dumps({"fetchedAt": case["stamp"], "payload": {}}), encoding="utf-8"
    )
    if case["epochMs"] is None:
        assert _load_cached(str(tmp_path), name, 0, 0) is None, case["name"]
        return
    fresh = _load_cached(str(tmp_path), name, 0, case["epochMs"])
    stale = _load_cached(str(tmp_path), name, 0, case["epochMs"] + 1)
    assert fresh is not None and fresh["stale"] is False, case["name"]
    assert stale is not None and stale["stale"] is True, case["name"]


@pytest.mark.parametrize("ms", _TS_CASES["write"], ids=lambda m: str(m))
def test_timestamp_write_conformance(ms, tmp_path) -> None:
    # the shared write contract: the facade writes an integral-ms, Z-suffixed,
    # 24-char stamp through its own cache-write path (_store_cached).
    from skopli import _load_cached, _store_cached

    name = "pricing-openrouter.json"
    stamp = _store_cached(str(tmp_path), name, {}, ms)
    assert len(stamp) == 24, stamp
    assert stamp.endswith("Z"), stamp
    fresh = _load_cached(str(tmp_path), name, 0, ms)
    stale = _load_cached(str(tmp_path), name, 0, ms + 1)
    assert fresh is not None and fresh["stale"] is False, stamp
    assert stale is not None and stale["stale"] is True, stamp


def _canonical(value):
    # canonicalize key order and number formatting for a raw-JSON-tree compare
    # (sort keys, round-trip numbers through JSON) without altering structure
    return json.loads(json.dumps(value, sort_keys=True))


def test_price_rollups_full_gold(tmp_path) -> None:
    gold = json.loads((PRICING_BASIC / "expected-priced-rollup.json").read_text(encoding="utf-8"))
    expected = gold["rollups"]
    pinned = "2026-08-01T00:00:00.000Z"

    # raw JSON copy of the expected rollups with ONLY `pricing` removed
    input_rollups = []
    for r in expected:
        r = dict(r)
        r.pop("pricing")
        input_rollups.append(r)

    catalogs = [
        {
            "source": "openrouter",
            "fetchedAt": pinned,
            "format": "openrouter",
            "payload": json.loads((CATALOGS / "openrouter.json").read_text(encoding="utf-8")),
        },
        {
            "source": "litellm",
            "fetchedAt": pinned,
            "format": "litellm",
            "payload": json.loads((CATALOGS / "litellm.json").read_text(encoding="utf-8")),
        },
    ]
    pricing = skopli.create_pricing(mode="calculate", catalogs=catalogs, sources=[])

    # drive the PUBLIC typed facade, then re-encode the returned
    # values back to a raw JSON tree and compare the COMPLETE tree against the
    # untouched expected rollups (canonicalized key order / number formatting)
    priced = pricing.price_rollups([skopli.Rollup.from_wire(r) for r in input_rollups])
    got = []
    for p in priced:
        wire = dict(p.rollup.to_wire())
        pricing_wire = _price_lookup_to_wire(p.pricing)
        if p.usd is not None:
            pricing_wire["usd"] = p.usd
        wire["pricing"] = pricing_wire
        got.append(wire)
    assert _canonical(got) == _canonical(expected)


def _price_lookup_to_wire(pricing) -> dict:
    # reconstruct the public typed lookup back to its wire shape so the gold
    # comparison exercises the facade round-trip for both hit and miss shapes
    if isinstance(pricing, PriceHit):
        out = {"priced": True, "model": pricing.model, "price": pricing.price.to_wire()}
        if pricing.key is not None:
            out["key"] = pricing.key
        out["source"] = pricing.source
        if pricing.fetched_at is not None:
            out["fetchedAt"] = pricing.fetched_at
        if getattr(pricing, "tiered_aggregate", None):
            out["tieredAggregate"] = True
        return out
    out = {"priced": False, "model": pricing.model, "attempted": list(pricing.attempted)}
    if pricing.reason is not None:
        out["reason"] = pricing.reason
    if pricing.key is not None:
        out["key"] = pricing.key
    return out


def test_lookup_model_hit_and_miss() -> None:
    pinned = "2026-08-01T00:00:00.000Z"
    catalogs = [
        {
            "source": "litellm",
            "fetchedAt": pinned,
            "format": "litellm",
            "payload": json.loads((CATALOGS / "litellm.json").read_text(encoding="utf-8")),
        },
    ]
    pricing = skopli.create_pricing(mode="calculate", catalogs=catalogs, sources=[])

    hit = pricing.lookup_model("gpt-5")
    assert isinstance(hit, PriceHit)
    assert hit.priced
    assert hit.source == "litellm"

    miss = pricing.lookup_model("totally-unknown-model-9000")
    assert isinstance(miss, PriceMiss)
    assert not miss.priced
