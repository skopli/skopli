package skopli

import (
	"encoding/json"
	"fmt"
	"sync"
	"time"
)

// PricingMode is the costing mode.
type PricingMode string

// Costing modes.
const (
	// ModeCalculate always computes cost from the catalog price.
	ModeCalculate PricingMode = "calculate"
	// ModeDisplay uses only the harness-recorded cost.
	ModeDisplay PricingMode = "display"
	// ModeAuto prefers the recorded cost, falling back to the calculated one.
	ModeAuto PricingMode = "auto"
)

// Override is a programmatic price override for a single model. It is the
// highest-priority catalog.
type Override struct {
	Model        string   `json:"model"`
	Input        float64  `json:"input"`
	Output       float64  `json:"output"`
	CacheRead    *float64 `json:"cacheRead,omitempty"`
	CacheWrite   *float64 `json:"cacheWrite,omitempty"`
	CacheWrite1h *float64 `json:"cacheWrite1h,omitempty"`
}

// Catalog is a pre-fetched pricing catalog handed to the core. Two forms are
// accepted (mirroring the C ABI ag_pricing_new contract):
//
//   - Prices set: an already-parsed {model: ModelPrice} map;
//   - Format + Payload set: a raw source payload the core parses ("openrouter",
//     "litellm", "modelsdev").
//
// A facade-side PricingSource produces one of these; see SourceContext.
type Catalog struct {
	Source    string                `json:"source"`
	FetchedAt string                `json:"fetchedAt,omitempty"`
	Prices    map[string]ModelPrice `json:"prices,omitempty"`
	Format    string                `json:"format,omitempty"`
	Payload   json.RawMessage       `json:"payload,omitempty"`
}

// SourceContext is passed to a PricingSource.Load. It carries the injectable
// fetch/clock seams plus the resolved cache lifecycle knobs. This is the
// facade-side seam: all fetching/caching lives in Go, and the resulting Catalog
// is handed down as data - no callback ever crosses the FFI.
type SourceContext struct {
	// Fetch retrieves the bytes at a URL, or an error. May be nil.
	Fetch func(url string) ([]byte, error)
	// Now is the clock used for cache freshness and fetch stamping.
	Now func() time.Time
	// CacheDir is the resolved on-disk cache directory.
	CacheDir string
	// Offline serves cache only; a source with no cache is skipped.
	Offline bool
	// TTLMs is the freshness window in milliseconds; age > TTLMs is stale.
	TTLMs int64
	// Refresh forces one fresh fetch, bypassing a fresh disk cache, with
	// stale-cache fallback if the fetch fails.
	Refresh bool
}

// PricingSource is a host-language pricing source. Name identifies it; Load
// produces a Catalog (or nil to contribute nothing) using the injectable seams
// in ctx. Implementations run entirely in Go.
type PricingSource interface {
	Name() string
	Load(ctx SourceContext) (*Catalog, error)
}

// PricingOptions configures NewPricing, at parity with the TS
// CreatePricingOptions. Sources are resolved facade-side (each Load runs in Go,
// fetching and caching in the host language) and their Catalogs are merged with
// any explicit Catalogs and Overrides before being handed to the core.
// BuiltinSources is always sent as false to the C ABI (which performs no network
// I/O). Priority highest-first: Overrides, then Catalogs, then the resolved
// Sources (default OpenRouter > LiteLLM > models.dev).
type PricingOptions struct {
	// Mode is the costing mode (defaults to ModeCalculate).
	Mode PricingMode
	// Overrides are programmatic per-model prices (highest priority).
	Overrides []Override
	// Catalogs are pre-fetched catalogs in priority order (after overrides).
	Catalogs []Catalog
	// Sources are host-language sources resolved facade-side before the FFI
	// call; each produced Catalog is appended after Catalogs. A nil slice
	// defaults to the built-in OpenRouter, LiteLLM, and models.dev sources; an
	// explicit empty slice contributes none.
	Sources []PricingSource
	// CacheDir is the on-disk cache directory; empty resolves to the platform
	// default via DefaultCacheDir.
	CacheDir string
	// Offline serves cache only; sources with no cache are skipped.
	Offline bool
	// TTLMs is the cache freshness window in milliseconds; zero defaults to 1h.
	TTLMs int64
	// Refresh forces one fresh fetch per source, bypassing a fresh disk cache,
	// with stale-cache fallback if the fetch fails.
	Refresh bool
	// Fetch is the injectable getter handed to each source's Load. Nil defaults
	// to a net/http getter.
	Fetch func(url string) ([]byte, error)
	// Now is the injectable clock (for deterministic tests). Nil defaults to
	// time.Now.
	Now func() time.Time
}

// DefaultTTLMs is the default cache freshness window (1 hour).
const DefaultTTLMs int64 = 60 * 60 * 1000

// nativePricingOptions is the JSON shape ag_pricing_new consumes.
type nativePricingOptions struct {
	Mode           string     `json:"mode,omitempty"`
	Overrides      []Override `json:"overrides,omitempty"`
	Catalogs       []Catalog  `json:"catalogs,omitempty"`
	BuiltinSources bool       `json:"builtinSources"`
}

// Pricing is a live pricing handle over a resolved catalog set. It owns the
// facade-side source lifecycle (fetch/cache/TTL/offline/refresh) and reloads
// its catalogs when the in-memory load expires (see live TTL reload below). A
// single mutex guards the native handle across every native call, the reload
// handle-swap, and Close, so concurrent queries and a concurrent Close cannot
// double-free or use the handle after free.
type Pricing struct {
	mu       sync.Mutex
	handle   *pricingHandle
	loadedAt *time.Time
	// closed is the terminal close state. It is distinct from loadedAt == nil
	// (a retryable failed load): once Close sets it, every query returns
	// ErrClosed before considering a reload, so a closed handle never
	// resurrects a native handle.
	closed bool

	mode      PricingMode
	overrides []Override
	catalogs  []Catalog
	sources   []PricingSource
	cacheDir  string
	offline   bool
	ttlMs     int64
	fetch     func(url string) ([]byte, error)
	now       func() time.Time

	// refresh is one-shot: it applies to the first load only. A failed refresh
	// load restores pendingRefresh so the retry honours it; sources whose
	// refresh fetch already completed are tracked so a retry does not refetch
	// them (mirrors src/pricing/index.ts).
	pendingRefresh bool
	refreshed      map[PricingSource]bool
}

// NewPricing builds a Pricing handle. Any PricingSources are resolved in Go
// first (each fetching/caching in the host language, their Catalogs merged in),
// then the combined overrides + catalogs are handed to the core; no callback
// crosses the FFI. Later queries on the same instance reload
// the catalogs once the in-memory load expires past TTLMs.
func NewPricing(opts PricingOptions) (*Pricing, error) {
	sources := opts.Sources
	if sources == nil {
		sources = builtinSources()
	}
	now := opts.Now
	if now == nil {
		now = time.Now
	}
	fetch := opts.Fetch
	if fetch == nil {
		fetch = httpFetch
	}
	ttlMs := opts.TTLMs
	if ttlMs == 0 {
		ttlMs = DefaultTTLMs
	}
	cacheDir := opts.CacheDir
	if cacheDir == "" {
		resolved, err := DefaultCacheDir()
		if err != nil {
			return nil, err
		}
		cacheDir = resolved
	}

	p := &Pricing{
		mode:           opts.Mode,
		overrides:      opts.Overrides,
		catalogs:       opts.Catalogs,
		sources:        sources,
		cacheDir:       cacheDir,
		offline:        opts.Offline,
		ttlMs:          ttlMs,
		fetch:          fetch,
		now:            now,
		pendingRefresh: opts.Refresh,
		refreshed:      make(map[PricingSource]bool),
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if err := p.load(); err != nil {
		return nil, err
	}
	return p, nil
}

// load resolves the source catalogs and builds a fresh native handle, swapping
// it in and freeing the previous one. It must be called with p.mu held. A
// failed load leaves loadedAt nil (not memoized) so the next query retries; a
// failed refresh load restores pendingRefresh so the retry honours it.
func (p *Pricing) load() error {
	refreshing := p.pendingRefresh
	p.pendingRefresh = false
	p.loadedAt = nil

	catalogs := make([]Catalog, 0, len(p.catalogs)+len(p.sources))
	catalogs = append(catalogs, p.catalogs...)
	ctx := SourceContext{
		Fetch:    p.fetch,
		Now:      p.now,
		CacheDir: p.cacheDir,
		Offline:  p.offline,
		TTLMs:    p.ttlMs,
	}
	for _, source := range p.sources {
		// per-source refresh: a retry after a partial failure skips sources
		// whose refresh fetch already completed
		ctx.Refresh = refreshing && !p.refreshed[source]
		catalog, err := source.Load(ctx)
		if err != nil {
			if refreshing {
				p.pendingRefresh = true
			}
			return fmt.Errorf("%w: source %q: %v", ErrCatalog, source.Name(), err)
		}
		if ctx.Refresh {
			p.refreshed[source] = true
		}
		if catalog != nil {
			if catalog.Source == "" {
				catalog.Source = source.Name()
			}
			catalogs = append(catalogs, *catalog)
		}
	}

	native := nativePricingOptions{
		Mode:           string(p.mode),
		Overrides:      p.overrides,
		Catalogs:       catalogs,
		BuiltinSources: false,
	}
	raw, err := json.Marshal(native)
	if err != nil {
		if refreshing {
			p.pendingRefresh = true
		}
		return fmt.Errorf("%w: marshalling pricing options: %v", ErrInvalidArgument, err)
	}
	handle, err := pricingNewRaw(raw)
	if err != nil {
		if refreshing {
			p.pendingRefresh = true
		}
		return err
	}
	p.handle.free()
	p.handle = handle
	loaded := p.now()
	p.loadedAt = &loaded
	return nil
}

// ensureFresh reloads the catalogs when the in-memory load has expired past
// TTLMs. Must be called with p.mu held. A closed handle returns ErrClosed
// before considering a reload (Close is terminal). The comparison is now -
// loadedAt >= ttlMs (deliberately distinct from disk-cache staleness, which is
// age > ttlMs). A failed load is not memoized (loadedAt stays
// nil), so the next query retries.
func (p *Pricing) ensureFresh() error {
	if p.closed {
		return ErrClosed
	}
	if p.loadedAt == nil {
		return p.load()
	}
	age := p.now().Sub(*p.loadedAt).Milliseconds()
	if age >= p.ttlMs {
		return p.load()
	}
	return nil
}

// Close frees the native handle. It is terminal: a subsequent query returns
// ErrClosed and never rebuilds a native handle. Safe to call multiple times and
// safe against a concurrent Close or query (the mutex serializes them).
func (p *Pricing) Close() {
	if p == nil {
		return
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	p.closed = true
	p.handle.free()
	p.handle = nil
	p.loadedAt = nil
}

// Catalogs returns the provenance of the loaded catalogs.
func (p *Pricing) Catalogs() ([]CatalogInfo, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if err := p.ensureFresh(); err != nil {
		return nil, err
	}
	out, err := p.handle.catalogInfoRaw()
	if err != nil {
		return nil, err
	}
	var infos []CatalogInfo
	if err := json.Unmarshal(out, &infos); err != nil {
		return nil, fmt.Errorf("%w: decoding catalog info: %v", ErrInternal, err)
	}
	return infos, nil
}

// PriceRollups prices a set of pre-aggregated rollups, returning each rollup
// with its pricing lookup attached (a hit with an optional tieredAggregate, or
// a miss). The input rollups are the same shape Rollup emits.
func (p *Pricing) PriceRollups(rollups []RollupBucket) ([]PricedRollup, error) {
	rollupsJSON, err := json.Marshal(rollups)
	if err != nil {
		return nil, fmt.Errorf("%w: marshalling rollups: %v", ErrInvalidArgument, err)
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if err := p.ensureFresh(); err != nil {
		return nil, err
	}
	out, err := p.handle.priceRollupsRaw(rollupsJSON)
	if err != nil {
		return nil, err
	}
	var priced []PricedRollup
	if err := json.Unmarshal(out, &priced); err != nil {
		return nil, fmt.Errorf("%w: decoding priced rollups: %v", ErrInternal, err)
	}
	return priced, nil
}

// LookupModel resolves a single model's price, returning a hit (Priced == true)
// or a miss (Priced == false).
func (p *Pricing) LookupModel(model string) (PriceLookup, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if err := p.ensureFresh(); err != nil {
		return PriceLookup{}, err
	}
	out, err := p.handle.lookupModelRaw([]byte(model))
	if err != nil {
		return PriceLookup{}, err
	}
	var lookup PriceLookup
	if err := json.Unmarshal(out, &lookup); err != nil {
		return PriceLookup{}, fmt.Errorf("%w: decoding lookup: %v", ErrInternal, err)
	}
	return lookup, nil
}

// PriceEvents prices an event batch grouped by the option dimension, returning
// each bucket with its pricing lookup attached.
func (p *Pricing) PriceEvents(events []UsageEvent, opts RollupOptions) ([]PricedRollup, error) {
	eventsJSON, err := json.Marshal(events)
	if err != nil {
		return nil, fmt.Errorf("%w: marshalling events: %v", ErrInvalidArgument, err)
	}
	optsJSON, err := json.Marshal(opts)
	if err != nil {
		return nil, fmt.Errorf("%w: marshalling options: %v", ErrInvalidArgument, err)
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if err := p.ensureFresh(); err != nil {
		return nil, err
	}
	out, err := p.handle.priceEventsRaw(eventsJSON, optsJSON)
	if err != nil {
		return nil, err
	}
	var priced []PricedRollup
	if err := json.Unmarshal(out, &priced); err != nil {
		return nil, fmt.Errorf("%w: decoding priced events: %v", ErrInternal, err)
	}
	return priced, nil
}
