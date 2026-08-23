# skopli (Python)

Native (PyO3 + maturin, abi3-py310) binding over `skopli-core`: read, roll
up, and price AI coding-agent token usage with no Node/Bun runtime.

```python
import skopli

detection = skopli.detect_harnesses()
result = skopli.read_usage(harnesses=["claude"], tz="UTC")
rollups = skopli.rollup(result.events, by="model", tz="UTC")

pricing = skopli.create_pricing(overrides=[
    {"model": "my-model", "input": 1.0, "output": 2.0},
])
priced = pricing.price_rollups(rollups)
```

The public API is snake_case; results are frozen slotted dataclasses; `Harness`
and `RollupBy` are string enums; a price lookup is a `PriceHit | PriceMiss`
union (`match` on `.priced`). Errors raise `SkopliError` ->
`InvalidArgumentError` (also a `ValueError`) / `CatalogError`.

Sync-only v1: every call releases the GIL, so wrap in `asyncio.to_thread` for
async callers.
