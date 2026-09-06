"""String enums for the public surface.

`enum.StrEnum` only exists on Python 3.11+, but the wheel targets abi3-py310, so
we provide a `str, enum.Enum` fallback that behaves identically for our uses
(value is the string, `==` against a plain string works, JSON-encodes as the
string via `str(...)`).
"""

from __future__ import annotations

import enum
import sys

if sys.version_info >= (3, 11):
    _StrEnum = enum.StrEnum
else:  # pragma: no cover - exercised only on 3.10 runtimes

    class _StrEnum(str, enum.Enum):
        """Minimal StrEnum backport: members are `str` subclasses."""

        def __str__(self) -> str:
            return str(self.value)


class Harness(_StrEnum):
    """A supported agent harness id (the wire `harness` value)."""

    AMP = "amp"
    AUGMENT = "augment"
    CHERRYSTUDIO = "cherrystudio"
    CLAUDE = "claude"
    CLINE = "cline"
    CODEBUDDY = "codebuddy"
    CODEBUFF = "codebuff"
    CODEX = "codex"
    COMMANDCODE = "commandcode"
    COPILOT = "copilot"
    DEEPSEEK = "deepseek"
    DEVIN = "devin"
    DROID = "droid"
    FX = "fx"
    GAJAE = "gajae"
    GEMINI = "gemini"
    GOOSE = "goose"
    GROK = "grok"
    HERMES = "hermes"
    JCODE = "jcode"
    JUNIE = "junie"
    KILO = "kilo"
    KILOCODE = "kilocode"
    KIMCHI = "kimchi"
    KIMI = "kimi"
    KIRO = "kiro"
    MIMOCODE = "mimocode"
    MUX = "mux"
    OMP = "omp"
    OPENCLAW = "openclaw"
    OPENCODE = "opencode"
    OPENCODEREVIEW = "opencodereview"
    PI = "pi"
    PRIME = "prime"
    QWEN = "qwen"
    REASONIX = "reasonix"
    ROO = "roo"
    TRAE = "trae"
    ZCODE = "zcode"
    ZED = "zed"


class RollupBy(_StrEnum):
    """A dimension a rollup groups by."""

    MODEL = "model"
    DAY = "day"
    SESSION = "session"
    HARNESS = "harness"
    WORKSPACE = "workspace"
    BLOCK = "block"


class PricingMode(_StrEnum):
    """How a `Pricing` engine derives per-rollup USD."""

    CALCULATE = "calculate"
    DISPLAY = "display"
    AUTO = "auto"


class Subagents(_StrEnum):
    """Whether to include or exclude subagent events in a read."""

    INCLUDE = "include"
    EXCLUDE = "exclude"
