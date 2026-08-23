package skopli

// Harness is the identifier of an agent harness (a string enum on the wire).
type Harness string

// Known harness identifiers. The list mirrors the registered core readers; a
// harness the core does not represent over this ABI is simply never emitted.
const (
	HarnessClaude         Harness = "claude"
	HarnessCodex          Harness = "codex"
	HarnessGemini         Harness = "gemini"
	HarnessOpencode       Harness = "opencode"
	HarnessMimocode       Harness = "mimocode"
	HarnessCommandcode    Harness = "commandcode"
	HarnessCopilot        Harness = "copilot"
	HarnessAmp            Harness = "amp"
	HarnessDroid          Harness = "droid"
	HarnessQwen           Harness = "qwen"
	HarnessPi             Harness = "pi"
	HarnessOmp            Harness = "omp"
	HarnessPrime          Harness = "prime"
	HarnessGajae          Harness = "gajae"
	HarnessKimchi         Harness = "kimchi"
	HarnessGrok           Harness = "grok"
	HarnessAugment        Harness = "augment"
	HarnessCodebuff       Harness = "codebuff"
	HarnessCodebuddy      Harness = "codebuddy"
	HarnessJcode          Harness = "jcode"
	HarnessMux            Harness = "mux"
	HarnessZcode          Harness = "zcode"
	HarnessRoo            Harness = "roo"
	HarnessCline          Harness = "cline"
	HarnessKilo           Harness = "kilo"
	HarnessKilocode       Harness = "kilocode"
	HarnessOpenclaw       Harness = "openclaw"
	HarnessKimi           Harness = "kimi"
	HarnessJunie          Harness = "junie"
	HarnessDevin          Harness = "devin"
	HarnessHermes         Harness = "hermes"
	HarnessGoose          Harness = "goose"
	HarnessZed            Harness = "zed"
	HarnessCherrystudio   Harness = "cherrystudio"
	HarnessOpencodereview Harness = "opencodereview"
	HarnessTrae           Harness = "trae"
	HarnessDeepseek       Harness = "deepseek"
	HarnessReasonix       Harness = "reasonix"
	HarnessKiro           Harness = "kiro"
	HarnessFx             Harness = "fx"
)

// KnownHarnesses lists every harness id the core registers, in registered
// order. It mirrors the shared registry-id fixture (golden/registry/ids.json)
// and is kept in sync by TestHarnessEnumCoversRegistry.
var KnownHarnesses = []Harness{
	HarnessClaude,
	HarnessCodex,
	HarnessGemini,
	HarnessOpencode,
	HarnessMimocode,
	HarnessCommandcode,
	HarnessCopilot,
	HarnessAmp,
	HarnessDroid,
	HarnessQwen,
	HarnessPi,
	HarnessOmp,
	HarnessPrime,
	HarnessGajae,
	HarnessKimchi,
	HarnessGrok,
	HarnessAugment,
	HarnessCodebuff,
	HarnessCodebuddy,
	HarnessJcode,
	HarnessMux,
	HarnessZcode,
	HarnessRoo,
	HarnessCline,
	HarnessKilo,
	HarnessKilocode,
	HarnessOpenclaw,
	HarnessKimi,
	HarnessJunie,
	HarnessDevin,
	HarnessHermes,
	HarnessGoose,
	HarnessZed,
	HarnessCherrystudio,
	HarnessOpencodereview,
	HarnessTrae,
	HarnessDeepseek,
	HarnessReasonix,
	HarnessKiro,
	HarnessFx,
}

// RollupBy is a rollup grouping dimension.
type RollupBy string

// Rollup dimensions accepted by Rollup and Pricing.PriceEvents.
const (
	ByModel     RollupBy = "model"
	ByDay       RollupBy = "day"
	BySession   RollupBy = "session"
	ByHarness   RollupBy = "harness"
	ByWorkspace RollupBy = "workspace"
	ByBlock     RollupBy = "block"
)

// DefaultBlockMs is the default billing-block width (five hours) used by ByBlock
// rollups when BlockMs is zero.
const DefaultBlockMs = 18_000_000

// TokenCounts is a bundle of token counts for one event or aggregated bucket.
// Optional cacheWrite1h is a pointer so it is omitted when absent (matching the
// core's absent-vs-null discipline).
type TokenCounts struct {
	Input        uint64  `json:"input"`
	Output       uint64  `json:"output"`
	CacheRead    uint64  `json:"cacheRead"`
	CacheWrite   uint64  `json:"cacheWrite"`
	CacheWrite1h *uint64 `json:"cacheWrite1h,omitempty"`
	Reasoning    uint64  `json:"reasoning"`
}

// UsageEvent is a single usage record read from a harness. Optional fields are
// pointers so they round-trip the wire's absent-vs-present distinction.
type UsageEvent struct {
	Harness   Harness     `json:"harness"`
	Timestamp string      `json:"timestamp"`
	SessionID string      `json:"sessionId"`
	MessageID string      `json:"messageId"`
	Turn      bool        `json:"turn"`
	Subagent  bool        `json:"subagent"`
	Model     string      `json:"model"`
	Tokens    TokenCounts `json:"tokens"`
	Calls     *uint64     `json:"calls,omitempty"`
	CostUSD   *float64    `json:"costUsd,omitempty"`
	Workspace *string     `json:"workspace,omitempty"`
	Title     *string     `json:"title,omitempty"`
}

// Diagnostic is a non-fatal warning surfaced during a read (malformed data never
// throws; it is reported here instead).
type Diagnostic struct {
	Severity string  `json:"severity"`
	Message  string  `json:"message"`
	Harness  Harness `json:"harness,omitempty"`
}

// ReadUsageResult is the {events, diagnostics, skipped} envelope returned by
// ReadUsage. Skipped maps a harness id to a list of skip reason strings.
type ReadUsageResult struct {
	Events      []UsageEvent         `json:"events"`
	Diagnostics []Diagnostic         `json:"diagnostics"`
	Skipped     map[Harness][]string `json:"skipped"`
}

// Detection is the result of DetectHarnesses: the harnesses whose data is
// readable, and the ones the tool knows about but cannot read over this ABI.
type Detection struct {
	Supported   []Harness `json:"supported"`
	Unsupported []Harness `json:"unsupported"`
}

// RollupBucket is one aggregated bucket. CostUSD is a pointer so it is omitted
// when absent (never serialized as null), matching the core.
//
// Named RollupBucket rather than Rollup because Go forbids a type and a
// function (the Rollup operation) sharing a name; the wire shape is unchanged.
type RollupBucket struct {
	Key     string      `json:"key"`
	Tokens  TokenCounts `json:"tokens"`
	Events  uint64      `json:"events"`
	Turns   uint64      `json:"turns"`
	Calls   uint64      `json:"calls"`
	CostUSD *float64    `json:"costUsd,omitempty"`
}

// PriceTier is one price tier of a tiered model price.
type PriceTier struct {
	Threshold    float64  `json:"threshold"`
	Input        float64  `json:"input"`
	Output       float64  `json:"output"`
	CacheRead    *float64 `json:"cacheRead,omitempty"`
	CacheWrite   *float64 `json:"cacheWrite,omitempty"`
	CacheWrite1h *float64 `json:"cacheWrite1h,omitempty"`
}

// ModelPrice is a per-million-token price for a model (flat, or tiered via
// Tiers). Rates are USD per million tokens.
type ModelPrice struct {
	Input        float64     `json:"input"`
	Output       float64     `json:"output"`
	CacheRead    *float64    `json:"cacheRead,omitempty"`
	CacheWrite   *float64    `json:"cacheWrite,omitempty"`
	CacheWrite1h *float64    `json:"cacheWrite1h,omitempty"`
	Tiers        []PriceTier `json:"tiers,omitempty"`
	TierMode     string      `json:"tierMode,omitempty"`
}

// CatalogInfo is the provenance of one loaded pricing catalog. FetchedAt is nil
// for the override catalog.
type CatalogInfo struct {
	Source    string  `json:"source"`
	FetchedAt *string `json:"fetchedAt"`
	Models    uint64  `json:"models"`
}

// PriceLookup is the priced-group pricing payload attached to each
// PricedRollup: a hit (Priced == true) or a miss (Priced == false). The
// discriminant Priced selects which fields are populated (see PriceHit /
// PriceMiss semantics in the pricing model).
type PriceLookup struct {
	Priced bool `json:"priced"`

	// Common / hit fields.
	Model     string      `json:"model,omitempty"`
	Key       string      `json:"key,omitempty"`
	Price     *ModelPrice `json:"price,omitempty"`
	Source    string      `json:"source,omitempty"`
	FetchedAt *string     `json:"fetchedAt,omitempty"`
	USD       *float64    `json:"usd,omitempty"`
	// TieredAggregate is set on a hit when a tiered model was aggregated across
	// multiple calls in the group.
	TieredAggregate bool `json:"tieredAggregate,omitempty"`
	// Models is set when a group priced multiple distinct models.
	Models []string `json:"models,omitempty"`

	// Miss fields.
	Attempted []string `json:"attempted,omitempty"`
	Reason    string   `json:"reason,omitempty"`
}

// PricedRollup is a Rollup bucket with its pricing lookup attached.
type PricedRollup struct {
	Key     string      `json:"key"`
	Tokens  TokenCounts `json:"tokens"`
	Events  uint64      `json:"events"`
	Turns   uint64      `json:"turns"`
	Calls   uint64      `json:"calls"`
	CostUSD *float64    `json:"costUsd,omitempty"`
	Pricing PriceLookup `json:"pricing"`
}
