"""Frozen slotted dataclasses for the public data surface, plus the wire
(de)serialization to/from the camelCase JSON the native module exchanges.

Every dataclass is ``frozen=True, slots=True``. Optional
fields are ``| None`` and omitted from the wire object when ``None`` (matching
the core's ``skip_serializing_if`` gold-file discipline). ``from_wire`` accepts
either a dataclass instance or a plain ``dict`` so callers can pass raw dicts.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Mapping, Sequence, Union

WireDict = dict[str, Any]


def _put(obj: WireDict, key: str, value: Any) -> None:
    if value is not None:
        obj[key] = value


@dataclass(frozen=True, slots=True)
class TokenCounts:
    input: int = 0
    output: int = 0
    cache_read: int = 0
    cache_write: int = 0
    reasoning: int = 0
    cache_write1h: int | None = None

    def to_wire(self) -> WireDict:
        obj: WireDict = {
            "input": self.input,
            "output": self.output,
            "cacheRead": self.cache_read,
            "cacheWrite": self.cache_write,
            "reasoning": self.reasoning,
        }
        _put(obj, "cacheWrite1h", self.cache_write1h)
        return obj

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "TokenCounts":
        return TokenCounts(
            input=int(d.get("input", 0)),
            output=int(d.get("output", 0)),
            cache_read=int(d.get("cacheRead", 0)),
            cache_write=int(d.get("cacheWrite", 0)),
            reasoning=int(d.get("reasoning", 0)),
            cache_write1h=(None if d.get("cacheWrite1h") is None else int(d["cacheWrite1h"])),
        )


@dataclass(frozen=True, slots=True)
class UsageEvent:
    harness: str
    timestamp: str
    session_id: str
    message_id: str
    turn: bool
    subagent: bool
    model: str
    tokens: TokenCounts
    calls: int | None = None
    cost_usd: float | None = None
    workspace: str | None = None
    title: str | None = None

    def to_wire(self) -> WireDict:
        obj: WireDict = {
            "harness": self.harness,
            "timestamp": self.timestamp,
            "sessionId": self.session_id,
            "messageId": self.message_id,
            "turn": self.turn,
            "subagent": self.subagent,
            "model": self.model,
            "tokens": self.tokens.to_wire(),
        }
        _put(obj, "calls", self.calls)
        _put(obj, "costUsd", self.cost_usd)
        _put(obj, "workspace", self.workspace)
        _put(obj, "title", self.title)
        return obj

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "UsageEvent":
        return UsageEvent(
            harness=d["harness"],
            timestamp=d["timestamp"],
            session_id=d["sessionId"],
            message_id=d["messageId"],
            turn=bool(d.get("turn", False)),
            subagent=bool(d.get("subagent", False)),
            model=d["model"],
            tokens=TokenCounts.from_wire(d["tokens"]),
            calls=d.get("calls"),
            cost_usd=d.get("costUsd"),
            workspace=d.get("workspace"),
            title=d.get("title"),
        )


EventLike = Union[UsageEvent, Mapping[str, Any]]


def event_to_wire(event: EventLike) -> WireDict:
    if isinstance(event, UsageEvent):
        return event.to_wire()
    return dict(event)


@dataclass(frozen=True, slots=True)
class Diagnostic:
    severity: str
    message: str
    harness: str | None = None

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "Diagnostic":
        return Diagnostic(
            severity=d["severity"],
            message=d["message"],
            harness=d.get("harness"),
        )


@dataclass(frozen=True, slots=True)
class ReadUsageResult:
    events: list[UsageEvent]
    diagnostics: list[Diagnostic]
    skipped: dict[str, list[str]]

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "ReadUsageResult":
        return ReadUsageResult(
            events=[UsageEvent.from_wire(e) for e in d.get("events", [])],
            diagnostics=[Diagnostic.from_wire(x) for x in d.get("diagnostics", [])],
            skipped={k: list(v) for k, v in d.get("skipped", {}).items()},
        )


@dataclass(frozen=True, slots=True)
class Detection:
    supported: list[str]
    unsupported: list[str]

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "Detection":
        return Detection(
            supported=list(d.get("supported", [])),
            unsupported=list(d.get("unsupported", [])),
        )


@dataclass(frozen=True, slots=True)
class Rollup:
    key: str
    tokens: TokenCounts
    events: int
    turns: int
    calls: int
    cost_usd: float | None = None

    def to_wire(self) -> WireDict:
        obj: WireDict = {
            "key": self.key,
            "tokens": self.tokens.to_wire(),
            "events": self.events,
            "turns": self.turns,
            "calls": self.calls,
        }
        _put(obj, "costUsd", self.cost_usd)
        return obj

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "Rollup":
        return Rollup(
            key=d["key"],
            tokens=TokenCounts.from_wire(d["tokens"]),
            events=int(d["events"]),
            turns=int(d["turns"]),
            calls=int(d["calls"]),
            cost_usd=d.get("costUsd"),
        )


RollupLike = Union[Rollup, Mapping[str, Any]]


def rollup_to_wire(r: RollupLike) -> WireDict:
    if isinstance(r, Rollup):
        return r.to_wire()
    return dict(r)


@dataclass(frozen=True, slots=True)
class PriceTier:
    threshold: float
    input: float
    output: float
    cache_read: float | None = None
    cache_write: float | None = None
    cache_write1h: float | None = None

    def to_wire(self) -> WireDict:
        obj: WireDict = {
            "threshold": self.threshold,
            "input": self.input,
            "output": self.output,
        }
        _put(obj, "cacheRead", self.cache_read)
        _put(obj, "cacheWrite", self.cache_write)
        _put(obj, "cacheWrite1h", self.cache_write1h)
        return obj

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "PriceTier":
        return PriceTier(
            threshold=float(d["threshold"]),
            input=float(d["input"]),
            output=float(d["output"]),
            cache_read=d.get("cacheRead"),
            cache_write=d.get("cacheWrite"),
            cache_write1h=d.get("cacheWrite1h"),
        )


@dataclass(frozen=True, slots=True)
class ModelPrice:
    input: float
    output: float
    cache_read: float | None = None
    cache_write: float | None = None
    cache_write1h: float | None = None
    tiers: list[PriceTier] | None = None
    tier_mode: str | None = None

    def to_wire(self) -> WireDict:
        obj: WireDict = {"input": self.input, "output": self.output}
        _put(obj, "cacheRead", self.cache_read)
        _put(obj, "cacheWrite", self.cache_write)
        _put(obj, "cacheWrite1h", self.cache_write1h)
        if self.tiers is not None:
            obj["tiers"] = [t.to_wire() for t in self.tiers]
        _put(obj, "tierMode", self.tier_mode)
        return obj

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "ModelPrice":
        tiers = d.get("tiers")
        return ModelPrice(
            input=float(d["input"]),
            output=float(d["output"]),
            cache_read=d.get("cacheRead"),
            cache_write=d.get("cacheWrite"),
            cache_write1h=d.get("cacheWrite1h"),
            tiers=None if tiers is None else [PriceTier.from_wire(t) for t in tiers],
            tier_mode=d.get("tierMode"),
        )


@dataclass(frozen=True, slots=True)
class PriceHit:
    """A successful price lookup. ``priced`` is always ``True`` (union tag)."""

    model: str
    key: str
    price: ModelPrice
    source: str
    fetched_at: str | None = None
    tiered_aggregate: bool = False
    priced: bool = field(default=True, init=False)

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "PriceHit":
        return PriceHit(
            model=d["model"],
            key=d["key"],
            price=ModelPrice.from_wire(d["price"]),
            source=d["source"],
            fetched_at=d.get("fetchedAt"),
            tiered_aggregate=bool(d.get("tieredAggregate", False)),
        )


@dataclass(frozen=True, slots=True)
class PriceMiss:
    """A failed price lookup. ``priced`` is always ``False`` (union tag)."""

    model: str
    attempted: list[str]
    reason: str | None = None
    key: str | None = None
    priced: bool = field(default=False, init=False)

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "PriceMiss":
        return PriceMiss(
            model=d["model"],
            attempted=list(d.get("attempted", [])),
            reason=d.get("reason"),
            key=d.get("key"),
        )


PriceLookup = Union[PriceHit, PriceMiss]


def price_lookup_from_wire(d: Mapping[str, Any]) -> PriceLookup:
    if d.get("priced"):
        return PriceHit.from_wire(d)
    return PriceMiss.from_wire(d)


@dataclass(frozen=True, slots=True)
class CatalogInfo:
    source: str
    models: int
    fetched_at: str | None = None

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "CatalogInfo":
        return CatalogInfo(
            source=d["source"],
            models=int(d["models"]),
            fetched_at=d.get("fetchedAt"),
        )


@dataclass(frozen=True, slots=True)
class PricedRollup:
    """A rollup with an attached ``pricing`` lookup (and derived ``usd``)."""

    rollup: Rollup
    pricing: PriceLookup
    usd: float | None

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "PricedRollup":
        pricing_obj = d["pricing"]
        return PricedRollup(
            rollup=Rollup.from_wire(d),
            pricing=price_lookup_from_wire(pricing_obj),
            usd=pricing_obj.get("usd"),
        )


@dataclass(frozen=True, slots=True)
class PricedEventGroup:
    """An event-group rollup with an attached ``pricing`` lookup and ``usd``."""

    rollup: Rollup
    pricing: PriceLookup
    usd: float | None

    @staticmethod
    def from_wire(d: Mapping[str, Any]) -> "PricedEventGroup":
        pricing_obj = d["pricing"]
        return PricedEventGroup(
            rollup=Rollup.from_wire(d),
            pricing=price_lookup_from_wire(pricing_obj),
            usd=pricing_obj.get("usd"),
        )


def catalog_to_wire(catalog: Mapping[str, Any]) -> WireDict:
    """Pass a pre-fetched catalog dict straight through (already wire-shaped)."""
    return dict(catalog)


def override_to_wire(entry: Mapping[str, Any]) -> WireDict:
    return dict(entry)


__all__ = [
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
    "EventLike",
    "RollupLike",
    "event_to_wire",
    "rollup_to_wire",
    "price_lookup_from_wire",
    "catalog_to_wire",
    "override_to_wire",
]

# `Sequence` is re-exported for annotations in __init__.
_ = Sequence
