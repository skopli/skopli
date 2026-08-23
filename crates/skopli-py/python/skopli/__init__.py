"""skopli - read, roll up, and price AI coding-agent token usage.

A typed, idiomatic Python facade over the native ``_skopli`` (PyO3) engine
that binds ``skopli-core``. The native layer exchanges JSON strings; this
facade owns the typed surface (frozen dataclasses, string enums, the
``PriceHit | PriceMiss`` union, the ``SkopliError`` hierarchy) and the
data-level seams (path overrides ``{home, env}`` and pre-fetched pricing
catalogs travel down as data; nothing crosses the FFI as a callback).

Sync-only v1: every native entry releases the GIL, so wrap calls in
``asyncio.to_thread`` for async callers.
"""

from __future__ import annotations

import json
import os
import tempfile
import threading
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Mapping, Sequence

from . import _skopli
from ._enums import Harness, PricingMode, RollupBy, Subagents
from ._errors import (
    SkopliError,
    CatalogError,
    InvalidArgumentError,
    raise_from_native,
)
from ._models import (
    CatalogInfo,
    Detection,
    Diagnostic,
    EventLike,
    ModelPrice,
    PriceHit,
    PriceLookup,
    PriceMiss,
    PriceTier,
    PricedEventGroup,
    PricedRollup,
    ReadUsageResult,
    Rollup,
    RollupLike,
    TokenCounts,
    UsageEvent,
    event_to_wire,
    price_lookup_from_wire,
    rollup_to_wire,
)

__version__ = _skopli.__version__

_NativeError = _skopli.SkopliError


def _resolve_context(
    home: str | None,
    env: Mapping[str, str] | None,
) -> dict[str, Any]:
    """Resolve the ``{home, env}`` path-override seam to concrete data.

    An explicit ``env`` mapping is the ENTIRE environment the readers see (the
    process environment is not consulted downstream), so when a caller omits it
    we snapshot ``os.environ`` here - matching the TS ``PathResolver`` default
    (the native side cannot see the parent process env).
    Empty-string values are dropped.
    """
    ctx: dict[str, Any] = {}
    resolved_home = home if home is not None else os.path.expanduser("~")
    if resolved_home:
        ctx["home"] = resolved_home
    source_env = env if env is not None else os.environ
    ctx["env"] = {k: v for k, v in source_env.items() if v}
    return ctx


def _call(fn, *args: str) -> Any:
    try:
        return fn(*args)
    except _NativeError as err:  # noqa: F841 - re-raised as typed subclass
        raise_from_native(err)


def default_cache_dir() -> str:
    """The default on-disk pricing-cache directory (platform-specific layout)."""
    return _skopli.default_cache_dir()


DEFAULT_TTL_MS = 60 * 60 * 1000

OPENROUTER_URL = "https://openrouter.ai/api/v1/models"
LITELLM_URL = (
    "https://raw.githubusercontent.com/BerriAI/litellm/main/"
    "model_prices_and_context_window.json"
)
MODELS_DEV_URL = "https://models.dev/api.json"

FETCH_TIMEOUT_S = 10.0

FetchLike = Callable[[str], Any]
ClockLike = Callable[[], float]


class SourceContext:
    """The context handed to a pricing source's ``load``.

    Carries the injectable ``fetch`` callable (``fetch(url) -> parsed JSON``,
    so a source does its own HTTP entirely in Python; no callback crosses the
    FFI), the on-disk cache directory, the freshness ``ttl_ms`` window, the
    ``offline`` / ``refresh`` flags, and the ``now_ms`` clock reading for this
    load. A source returns a wire catalog entry ``{source, fetchedAt, format,
    payload}`` or a pre-parsed ``{source, fetchedAt, prices}``, or ``None``.
    """

    __slots__ = ("fetch", "cache_dir", "ttl_ms", "offline", "refresh", "now_ms")

    def __init__(
        self,
        *,
        fetch: FetchLike,
        cache_dir: str,
        ttl_ms: float,
        offline: bool,
        refresh: bool,
        now_ms: float,
    ) -> None:
        self.fetch = fetch
        self.cache_dir = cache_dir
        self.ttl_ms = ttl_ms
        self.offline = offline
        self.refresh = refresh
        self.now_ms = now_ms


class BuiltinSource:
    """A built-in market source: a wire ``source`` name, a core parser
    ``format``, and the URL its raw payload is fetched from.

    Implements the ``load(context)`` source protocol: it resolves its own disk
    cache, fetches when needed, validates every payload through the core parser
    (never trusting a payload that parses to zero prices), and returns a wire
    catalog entry or ``None``. A user-supplied object with the same
    ``load(context)`` method is driven identically.
    """

    __slots__ = ("name", "format", "url")

    def __init__(self, name: str, format: str, url: str) -> None:
        self.name = name
        self.format = format
        self.url = url

    def load(self, context: SourceContext) -> dict[str, Any] | None:
        return _load_source(
            self,
            cache_dir=context.cache_dir,
            ttl_ms=context.ttl_ms,
            offline=context.offline,
            refresh=context.refresh,
            fetch=context.fetch,
            now_ms=context.now_ms,
        )


def _builtin_sources() -> list[BuiltinSource]:
    return [
        BuiltinSource("openrouter", "openrouter", OPENROUTER_URL),
        BuiltinSource("litellm", "litellm", LITELLM_URL),
        BuiltinSource("models-dev", "modelsdev", MODELS_DEV_URL),
    ]


def _cache_file_name(name: str) -> str:
    # the on-disk cache filename a built-in source reads and writes; the single
    # production formula, exposed for the constants contract test in golden/pricing/lifecycle
    return f"pricing-{name}.json"


def _iso(now_ms: float) -> str:
    dt = datetime.fromtimestamp(now_ms / 1000, tz=timezone.utc)
    return dt.strftime("%Y-%m-%dT%H:%M:%S.") + f"{dt.microsecond // 1000:03d}Z"


def _load_cached(
    cache_dir: str, name: str, ttl_ms: float, now_ms: float
) -> dict[str, Any] | None:
    """Read ``<cache_dir>/<name>`` in the byte-compatible cache format.

    Returns ``{fetchedAt, payload, stale}`` or ``None`` when absent/malformed.
    ``stale`` is ``age > ttl_ms`` (age == ttl_ms is fresh, matching the shared
    cross-facade contract and the TS/Rust reference).
    """
    try:
        raw = Path(cache_dir, name).read_text(encoding="utf-8")
    except OSError:
        return None
    try:
        parsed = json.loads(raw)
    except (ValueError, TypeError):
        return None
    if not isinstance(parsed, dict):
        return None
    fetched_at = parsed.get("fetchedAt")
    if not isinstance(fetched_at, str) or "payload" not in parsed:
        return None
    try:
        stamp = datetime.fromisoformat(fetched_at.replace("Z", "+00:00"))
    except ValueError:
        return None
    age = now_ms - stamp.timestamp() * 1000
    return {"fetchedAt": fetched_at, "payload": parsed["payload"], "stale": age > ttl_ms}


def _store_cached(cache_dir: str, name: str, payload: Any, now_ms: float) -> str:
    """Write the cache file atomically (temp sibling, then ``os.replace``).

    Returns the ISO-8601 UTC ``fetchedAt`` stamped into the file.
    """
    fetched_at = _iso(now_ms)
    body = json.dumps({"fetchedAt": fetched_at, "payload": payload})
    os.makedirs(cache_dir, exist_ok=True)
    # a genuinely unique sibling per write (exclusive-create temp) so concurrent
    # writers of the same cache never target the same path and defeat the
    # atomic replace; the fd is closed before os.replace
    fd, tmp = tempfile.mkstemp(prefix=f"{name}.", suffix=".tmp", dir=cache_dir)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write(body)
        os.replace(tmp, os.path.join(cache_dir, name))
    except OSError:
        try:
            os.remove(tmp)
        finally:
            pass
        raise
    return fetched_at


def _default_fetch_json(url: str) -> Any:
    # a stalled market endpoint must never hang an otherwise local run; a
    # non-2xx status is a fetch failure, not a payload (urlopen raises
    # HTTPError for >= 400, so the >= 300 guard covers the rest)
    with urllib.request.urlopen(  # noqa: S310 - caller-supplied url
        url, timeout=FETCH_TIMEOUT_S
    ) as response:
        status = getattr(response, "status", None)
        if status is not None and not (200 <= status < 300):
            raise RuntimeError(f"{url} responded {status}")
        return json.loads(response.read())


def _probe_prices(entry: dict[str, Any]) -> int:
    """Count parsed prices in a single raw catalog entry via the core parser.

    Builds a throwaway native pricing handle whose options contain exactly this
    one raw catalog (wire grammar, ``builtinSources: false``, mode
    ``calculate``), reads the catalog's ``models`` count, and lets the handle
    be freed. Any error from the probe build means the payload is unusable, so
    it counts as zero. This matches the TS "zero parsed prices is unusable"
    rule without reimplementing a source parser in Python.
    """
    opts = {
        "mode": "calculate",
        "builtin_sources": False,
        "catalogs": [entry],
    }
    try:
        native = _skopli.Pricing(json.dumps(opts))
        infos = json.loads(native.catalogs())
    except Exception:
        return 0
    for info in infos:
        if info.get("source") == entry["source"]:
            return int(info.get("models", 0))
    return 0


def _load_source(
    source: BuiltinSource,
    *,
    cache_dir: str,
    ttl_ms: float,
    offline: bool,
    refresh: bool,
    fetch: FetchLike,
    now_ms: float,
) -> dict[str, Any] | None:
    """Resolve one built-in source to a wire catalog entry, or ``None``.

    Mirrors the TS ``cachedSource.load``: serve a fresh cache (or any cache when
    offline) without fetching, otherwise fetch and cache, falling back to a stale
    cache when the fetch fails or is skipped offline. Every cached/fetched
    payload is validated through the core parser (:func:`_probe_prices`): a
    payload that parses to zero prices is unusable, exactly as TS treats a
    zero-size price map.
    """
    cache_name = _cache_file_name(source.name)
    cached = _load_cached(cache_dir, cache_name, ttl_ms, now_ms)

    def entry(fetched_at: str | None, payload: Any) -> dict[str, Any]:
        return {
            "source": source.name,
            "fetchedAt": fetched_at,
            "format": source.format,
            "payload": payload,
        }

    # a cache payload that probes to zero prices is unusable, same as no cache
    cached_catalog = None
    if cached is not None:
        candidate = entry(cached["fetchedAt"], cached["payload"])
        if _probe_prices(candidate) > 0:
            cached_catalog = candidate
    cache_fresh = (
        cached is not None
        and not cached["stale"]
        and not refresh
        and cached_catalog is not None
    )
    if cached_catalog is not None and (cache_fresh or offline):
        return cached_catalog
    if offline:
        return None
    try:
        payload = fetch(source.url)
    except Exception:
        # any-age stale fallback keeps pricing available when the network is not
        return cached_catalog
    # a fetched payload that probes to zero prices (or is not valid catalog
    # data) is a fetch failure: do NOT write the cache, serve the prior usable
    # cache if any, else skip the source
    fetched = entry(None, payload)
    if _probe_prices(fetched) == 0:
        return cached_catalog
    fetched_at = _store_cached(cache_dir, cache_name, payload, now_ms)
    return entry(fetched_at, payload)


def detect_harnesses(
    *,
    home: str | None = None,
    env: Mapping[str, str] | None = None,
) -> Detection:
    """Detect which harnesses have readable data under the given context."""
    opts = _resolve_context(home, env)
    raw = _call(_skopli.detect_harnesses, json.dumps(opts))
    return Detection.from_wire(json.loads(raw))


def read_usage(
    *,
    home: str | None = None,
    env: Mapping[str, str] | None = None,
    harnesses: Sequence[str] | None = None,
    since: str | None = None,
    until: str | None = None,
    tz: str | None = None,
    subagents: str | None = None,
) -> ReadUsageResult:
    """Read usage across the selected harnesses.

    ``since``/``until`` are date (``YYYY-MM-DD``, UTC midnight) or ISO strings;
    ``tz`` is an IANA timezone for day bucketing (defaults to UTC); ``subagents``
    is ``"exclude"`` to drop subagent events. Returns ``{events, diagnostics,
    skipped}``.
    """
    opts = _resolve_context(home, env)
    if harnesses is not None:
        opts["harnesses"] = [str(h) for h in harnesses]
    if since is not None:
        opts["since"] = since
    if until is not None:
        opts["until"] = until
    if tz is not None:
        opts["tz"] = tz
    if subagents is not None:
        opts["subagents"] = str(subagents)
    raw = _call(_skopli.read_usage, json.dumps(opts))
    return ReadUsageResult.from_wire(json.loads(raw))


def rollup(
    events: Sequence[EventLike],
    by: str,
    *,
    tz: str | None = None,
    block_ms: int | None = None,
) -> list[Rollup]:
    """Roll up a batch of events by a dimension (``model``/``day``/``block``/...).

    ``block_ms`` sets the billing-block width for ``by="block"`` (defaults to
    five hours); it is ignored for other dimensions.
    """
    events_json = json.dumps([event_to_wire(e) for e in events])
    opts: dict[str, Any] = {"by": str(by)}
    if tz is not None:
        opts["tz"] = tz
    if block_ms is not None:
        opts["blockMs"] = block_ms
    raw = _call(_skopli.rollup, events_json, json.dumps(opts))
    return [Rollup.from_wire(r) for r in json.loads(raw)]


def cost_usd(tokens: TokenCounts | Mapping[str, Any], price: ModelPrice | Mapping[str, Any]) -> float:
    """Compute the USD cost of a token bundle under a flat/tiered price."""
    tokens_json = json.dumps(tokens.to_wire() if isinstance(tokens, TokenCounts) else dict(tokens))
    price_json = json.dumps(price.to_wire() if isinstance(price, ModelPrice) else dict(price))
    return _call(_skopli.cost_usd, tokens_json, price_json)


class _PricingLoader:
    """The retained lifecycle config for a :class:`Pricing` instance.

    Holds the resolved sources, base native options (mode + overrides +
    explicit catalogs), and the fetch/cache/offline/refresh/ttl/clock knobs, so
    a TTL-expiry reload can rerun the source lifecycle and rebuild the native
    handle from the same inputs. ``refresh`` is one-shot: it applies to the
    first load only; per-source completion is tracked so a partial-failure
    retry does not refetch a source whose refresh already completed.
    """

    __slots__ = (
        "sources",
        "base_opts",
        "cache_dir",
        "ttl_ms",
        "offline",
        "fetch",
        "clock",
        "pending_refresh",
        "refreshed",
    )

    def __init__(
        self,
        *,
        sources: list[Any],
        base_opts: dict[str, Any],
        cache_dir: str,
        ttl_ms: float,
        offline: bool,
        fetch: FetchLike,
        clock: ClockLike,
        refresh: bool,
    ) -> None:
        self.sources = sources
        self.base_opts = base_opts
        self.cache_dir = cache_dir
        self.ttl_ms = ttl_ms
        self.offline = offline
        self.fetch = fetch
        self.clock = clock
        self.pending_refresh = refresh
        self.refreshed: set[int] = set()

    def build(self) -> Any:
        """Run the source lifecycle at the current clock and build a native
        handle. A rejected build/refresh is not memoized here; the caller only
        records ``loadedAt`` when this returns, so a raised error retries next
        query and a failed refresh stays pending."""
        now_ms = self.clock()
        refreshing = self.pending_refresh
        opts = dict(self.base_opts)
        loaded: list[dict[str, Any]] = list(opts.get("catalogs", []))
        for source in self.sources:
            # per-source refresh: a retry after a partial failure skips sources
            # whose refresh fetch already completed
            refresh = refreshing and id(source) not in self.refreshed
            context = SourceContext(
                fetch=self.fetch,
                cache_dir=self.cache_dir,
                ttl_ms=self.ttl_ms,
                offline=self.offline,
                refresh=refresh,
                now_ms=now_ms,
            )
            catalog = source.load(context)
            if refresh:
                self.refreshed.add(id(source))
            if catalog is not None:
                loaded.append(catalog)
        if loaded:
            opts["catalogs"] = loaded
        opts["builtin_sources"] = False
        try:
            native = _skopli.Pricing(json.dumps(opts))
        except _NativeError as err:
            raise_from_native(err)
        # only clear the one-shot refresh once the load COMPLETED
        self.pending_refresh = False
        return native


class Pricing:
    """A pricing engine over a set of catalogs with a live lifecycle.

    Built by :func:`create_pricing`. Retains its resolved sources, options, and
    a ``loadedAt`` stamp from the injected clock; before every query, if the
    load has aged past ``ttl_ms`` it reruns the source lifecycle (each source's
    own disk cache keeps this cheap when still fresh), builds a fresh native
    handle, and swaps it in. All native calls and the reload/swap run under one
    lock (native FFI releases the GIL, so a flag check alone would be racy).
    """

    __slots__ = ("_native", "_loader", "_ttl_ms", "_clock", "_loaded_at", "_lock")

    def __init__(self, native: Any, loader: _PricingLoader) -> None:
        self._native = native
        self._loader = loader
        self._ttl_ms = loader.ttl_ms
        self._clock = loader.clock
        self._loaded_at: float = loader.clock()
        self._lock = threading.Lock()

    def _ensure_fresh(self) -> None:
        # caller holds self._lock. Reload when the in-memory load has aged past
        # the TTL window (now - loadedAt >= ttlMs; note this differs from the
        # disk-cache `age > ttlMs` staleness by design). A failed reload is not
        # memoized: loadedAt is only advanced when build() returns.
        if self._loaded_at is None:
            return
        if self._clock() - self._loaded_at >= self._ttl_ms:
            native = self._loader.build()
            self._native = native
            self._loaded_at = self._clock()

    def catalogs(self) -> list[CatalogInfo]:
        """Provenance of the loaded catalogs."""
        with self._lock:
            self._ensure_fresh()
            raw = _call(self._native.catalogs)
        return [CatalogInfo.from_wire(c) for c in json.loads(raw)]

    def lookup_model(self, model: str) -> PriceLookup:
        """Look up a model's price: a ``PriceHit | PriceMiss`` (match on ``.priced``)."""
        with self._lock:
            self._ensure_fresh()
            raw = _call(self._native.lookup_model, model)
        return price_lookup_from_wire(json.loads(raw))

    def price_tokens(self, tokens: TokenCounts | Mapping[str, Any], model: str) -> float:
        """Cost a token bundle for ``model`` under this engine's catalogs.

        Facade-side sugar over :meth:`lookup_model` + :func:`cost_usd`: a miss
        costs 0.0 (no price to apply).
        """
        lookup = self.lookup_model(model)
        if isinstance(lookup, PriceHit):
            return cost_usd(tokens, lookup.price)
        return 0.0

    def price_rollups(self, rollups: Sequence[RollupLike]) -> list[PricedRollup]:
        """Attach pricing to a set of rollups."""
        payload = json.dumps([rollup_to_wire(r) for r in rollups])
        with self._lock:
            self._ensure_fresh()
            raw = _call(self._native.price_rollups, payload)
        return [PricedRollup.from_wire(r) for r in json.loads(raw)]

    def price_events(
        self,
        events: Sequence[EventLike],
        by: str,
        *,
        tz: str | None = None,
        block_ms: int | None = None,
    ) -> list[PricedEventGroup]:
        """Price a batch of events grouped by a rollup dimension.

        ``block_ms`` sets the billing-block width for ``by="block"`` (defaults
        to five hours); it is ignored for other dimensions.
        """
        events_json = json.dumps([event_to_wire(e) for e in events])
        opts: dict[str, Any] = {"by": str(by)}
        if tz is not None:
            opts["tz"] = tz
        if block_ms is not None:
            opts["blockMs"] = block_ms
        with self._lock:
            self._ensure_fresh()
            raw = _call(self._native.price_events, events_json, json.dumps(opts))
        return [PricedEventGroup.from_wire(g) for g in json.loads(raw)]


def create_pricing(
    *,
    mode: str | None = None,
    overrides: Sequence[Mapping[str, Any]] | None = None,
    catalogs: Sequence[Mapping[str, Any]] | None = None,
    sources: Sequence[Any] | None = None,
    cache_dir: str | None = None,
    offline: bool = False,
    ttl_ms: float | None = None,
    refresh: bool = False,
    fetch: FetchLike | None = None,
    _clock: ClockLike | None = None,
) -> Pricing:
    """Build a :class:`Pricing` engine with the full built-in pricing lifecycle.

    The facade owns fetching and the on-disk cache; the native core does
    matching + costing only and never touches the network (``builtin_sources``
    is always sent as ``False`` on the wire, the direct-core seam contract).

    Options mirror the TS ``createPricing``:

    - ``overrides``: highest-priority ``{model, input, output, cacheRead?, ...}``.
    - ``catalogs``: pre-fetched catalogs, each ``{source, fetchedAt?, prices}``
      or ``{source, fetchedAt?, format, payload}`` (format one of ``openrouter``
      / ``litellm`` / ``modelsdev``); highest priority after overrides.
    - ``sources``: priority-ordered pricing sources, resolved HERE in Python;
      each is any object with a ``load(context)`` method (the three built-ins,
      :class:`BuiltinSource`, are implementations of it, and a custom source
      with the same method is driven identically). Defaults to
      OpenRouter > LiteLLM > models.dev; pass ``sources=[]`` for no network.
    - ``cache_dir``: on-disk cache directory (defaults to
      :func:`default_cache_dir`).
    - ``offline``: serve from cache only, never fetch.
    - ``ttl_ms``: cache freshness window (default 1h). Age ``> ttl_ms`` is stale.
    - ``refresh``: force one fresh fetch per source, with stale-cache fallback.
    - ``fetch``: injectable ``fetch(url) -> parsed JSON`` seam for tests.

    ``mode`` is ``calculate`` (default) / ``display`` / ``auto``.
    """
    resolved_sources = _builtin_sources() if sources is None else list(sources)
    resolved_cache_dir = cache_dir if cache_dir is not None else default_cache_dir()
    resolved_ttl = DEFAULT_TTL_MS if ttl_ms is None else ttl_ms
    fetch_impl = fetch if fetch is not None else _default_fetch_json
    clock = _clock if _clock is not None else (lambda: datetime.now(timezone.utc).timestamp() * 1000)

    base_opts: dict[str, Any] = {}
    if mode is not None:
        base_opts["mode"] = str(mode)
    if overrides:
        base_opts["overrides"] = [dict(o) for o in overrides]
    if catalogs:
        base_opts["catalogs"] = [dict(c) for c in catalogs]

    loader = _PricingLoader(
        sources=resolved_sources,
        base_opts=base_opts,
        cache_dir=resolved_cache_dir,
        ttl_ms=resolved_ttl,
        offline=offline,
        fetch=fetch_impl,
        clock=clock,
        refresh=refresh,
    )
    native = loader.build()
    return Pricing(native, loader)


__all__ = [
    "__version__",
    "SkopliError",
    "CatalogError",
    "InvalidArgumentError",
    "Harness",
    "RollupBy",
    "PricingMode",
    "Subagents",
    "TokenCounts",
    "UsageEvent",
    "Diagnostic",
    "ReadUsageResult",
    "Detection",
    "Rollup",
    "PriceTier",
    "ModelPrice",
    "PriceHit",
    "PriceMiss",
    "PriceLookup",
    "CatalogInfo",
    "PricedRollup",
    "PricedEventGroup",
    "Pricing",
    "BuiltinSource",
    "SourceContext",
    "detect_harnesses",
    "read_usage",
    "rollup",
    "cost_usd",
    "create_pricing",
    "default_cache_dir",
    "OPENROUTER_URL",
    "LITELLM_URL",
    "MODELS_DEV_URL",
    "DEFAULT_TTL_MS",
]
