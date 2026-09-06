// Pricing lifecycle + new-export conformance, driven by the SHARED fixtures in
// golden/pricing/lifecycle/ (the five-behavior matrix) and golden/pricing/basic/
// (rollup gold equality, lookup hit + miss). A facade that drifts from the TS
// reference fails against the shared gold rather than its author's assumptions.
package skopli

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"
)

func lifecycleDir(t *testing.T) string {
	return filepath.Join(repoRoot(t), "golden", "pricing", "lifecycle")
}

func loadLifecycleFixture(t *testing.T, v any, segments ...string) {
	t.Helper()
	p := filepath.Join(append([]string{lifecycleDir(t)}, segments...)...)
	b, err := os.ReadFile(p)
	if err != nil {
		t.Fatalf("read fixture %v: %v", segments, err)
	}
	if err := json.Unmarshal(b, v); err != nil {
		t.Fatalf("decode fixture %v: %v", segments, err)
	}
}

type lifecycleRequest struct {
	Source           string                     `json:"source"`
	CacheFileName    string                     `json:"cacheFileName"`
	TTLMs            int64                      `json:"ttlMs"`
	Now              string                     `json:"now"`
	FetchedAtFresh   string                     `json:"fetchedAtFresh"`
	FetchedAtStale   string                     `json:"fetchedAtStale"`
	FetchedAtFetched string                     `json:"fetchedAtFetched"`
	NowSecond        string                     `json:"nowSecond"`
	Options          struct{ Mode PricingMode } `json:"options"`
	Rollup           RollupBucket               `json:"rollup"`
}

type lifecycleExpected struct {
	Behavior  string  `json:"behavior"`
	Fetched   bool    `json:"fetched"`
	FetchedAt *string `json:"fetchedAt"`
	Priced    bool    `json:"priced"`
	PricedUSD float64 `json:"pricedUsd"`
}

func loadLifecycleExpected(t *testing.T, behavior string) lifecycleExpected {
	t.Helper()
	var expected lifecycleExpected
	loadLifecycleFixture(t, &expected, "expected", behavior+".json")
	if expected.Behavior != behavior {
		t.Fatalf("behavior mismatch: got %q want %q", expected.Behavior, behavior)
	}
	return expected
}

// recordingFetch counts calls, then serves payload (or errors when throws).
type recordingFetch struct {
	payload []byte
	throws  bool
	calls   int
}

func (r *recordingFetch) fetch(string) ([]byte, error) {
	r.calls++
	if r.throws {
		return nil, errNetworkDown
	}
	return r.payload, nil
}

var errNetworkDown = &fetchError{"network down"}

type fetchError struct{ msg string }

func (e *fetchError) Error() string { return e.msg }

func fixedClock(t *testing.T, iso string) func() time.Time {
	t.Helper()
	parsed, err := time.Parse(time.RFC3339Nano, iso)
	if err != nil {
		t.Fatalf("parse clock %q: %v", iso, err)
	}
	return func() time.Time { return parsed }
}

func installCache(t *testing.T, dir, name, fixture string) {
	t.Helper()
	var cache json.RawMessage
	loadLifecycleFixture(t, &cache, fixture)
	if err := os.WriteFile(filepath.Join(dir, name), cache, 0o644); err != nil {
		t.Fatalf("install cache: %v", err)
	}
}

// sourcePayload returns the canonical OpenRouter source payload the lifecycle
// matrix fetches, consumed from source-openrouter.json (the README-named
// canonical payload) rather than derived from cache-fresh.json.
func sourcePayload(t *testing.T) []byte {
	t.Helper()
	var payload json.RawMessage
	loadLifecycleFixture(t, &payload, "source-openrouter.json")
	return payload
}

// sourceFactory selects the built-in source factory named by request.source, so
// the matrix consumes the fixture-declared source instead of hardcoding one.
func sourceFactory(t *testing.T, name string) func(url string) PricingSource {
	t.Helper()
	switch name {
	case "openrouter":
		return OpenRouterSource
	case "litellm":
		return LiteLLMSource
	case "models-dev":
		return ModelsDevSource
	default:
		t.Fatalf("unknown source %q", name)
		return nil
	}
}

func currentFetchedAt(t *testing.T, dir, name string) *string {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(dir, name))
	if err != nil {
		return nil
	}
	var file cacheFile
	if err := json.Unmarshal(b, &file); err != nil {
		t.Fatalf("decode cache: %v", err)
	}
	return &file.FetchedAt
}

const sourceURL = "https://or.test/models"

// runLifecycle drives one behavior: builds the fixture-declared source over the
// injected fetch/clock and a temp cacheDir, prices the shared rollup, and
// returns the priced rollup. The fetch count is asserted on stub separately.
func runLifecycle(t *testing.T, req lifecycleRequest, dir string, stub *recordingFetch, offline, refresh bool) PricedRollup {
	t.Helper()
	make := sourceFactory(t, req.Source)
	pricing, err := NewPricing(PricingOptions{
		Mode:     req.Options.Mode,
		Sources:  []PricingSource{make(sourceURL)},
		CacheDir: dir,
		TTLMs:    req.TTLMs,
		Offline:  offline,
		Refresh:  refresh,
		Fetch:    stub.fetch,
		Now:      fixedClock(t, req.Now),
	})
	if err != nil {
		t.Fatalf("NewPricing: %v", err)
	}
	defer pricing.Close()
	priced, err := pricing.PriceRollups([]RollupBucket{req.Rollup})
	if err != nil {
		t.Fatalf("PriceRollups: %v", err)
	}
	if len(priced) != 1 {
		t.Fatalf("want 1 priced rollup, got %d", len(priced))
	}
	return priced[0]
}

func assertLifecycle(t *testing.T, exp lifecycleExpected, stub *recordingFetch, got PricedRollup, dir, name string) {
	t.Helper()
	wantCalls := 0
	if exp.Fetched {
		wantCalls = 1
	}
	if stub.calls != wantCalls {
		t.Fatalf("%s: fetch calls = %d, want %d", exp.Behavior, stub.calls, wantCalls)
	}
	if got.Pricing.Priced != exp.Priced {
		t.Fatalf("%s: priced = %v, want %v", exp.Behavior, got.Pricing.Priced, exp.Priced)
	}
	usd := 0.0
	if got.Pricing.USD != nil {
		usd = *got.Pricing.USD
	}
	if !closeEnough(usd, exp.PricedUSD) {
		t.Fatalf("%s: usd = %v, want %v", exp.Behavior, usd, exp.PricedUSD)
	}
	stamp := currentFetchedAt(t, dir, name)
	if exp.FetchedAt == nil {
		if stamp != nil {
			t.Fatalf("%s: expected no cache file, got fetchedAt %q", exp.Behavior, *stamp)
		}
		return
	}
	if stamp == nil {
		t.Fatalf("%s: expected cache fetchedAt %q, got none", exp.Behavior, *exp.FetchedAt)
	}
	if *stamp != *exp.FetchedAt {
		t.Fatalf("%s: fetchedAt = %q, want %q", exp.Behavior, *stamp, *exp.FetchedAt)
	}
}

func closeEnough(a, b float64) bool {
	d := a - b
	if d < 0 {
		d = -d
	}
	return d < 1e-9
}

func TestLifecycleColdFetch(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	exp := loadLifecycleExpected(t, "cold-fetch")
	dir := t.TempDir()
	stub := &recordingFetch{payload: sourcePayload(t)}
	got := runLifecycle(t, req, dir, stub, false, false)
	assertLifecycle(t, exp, stub, got, dir, req.CacheFileName)
	if *exp.FetchedAt != req.FetchedAtFetched {
		t.Fatalf("expected fetchedAt to equal fetchedAtFetched %q", req.FetchedAtFetched)
	}
}

func TestLifecycleWarmCache(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	exp := loadLifecycleExpected(t, "warm-cache")
	dir := t.TempDir()
	installCache(t, dir, req.CacheFileName, "cache-fresh.json")
	stub := &recordingFetch{payload: sourcePayload(t)}
	got := runLifecycle(t, req, dir, stub, false, false)
	assertLifecycle(t, exp, stub, got, dir, req.CacheFileName)
	if *exp.FetchedAt != req.FetchedAtFresh {
		t.Fatalf("expected fetchedAt to equal fetchedAtFresh %q", req.FetchedAtFresh)
	}
}

func TestLifecycleTTLRefresh(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	exp := loadLifecycleExpected(t, "ttl-refresh")
	dir := t.TempDir()
	installCache(t, dir, req.CacheFileName, "cache-stale.json")
	stub := &recordingFetch{payload: sourcePayload(t)}
	got := runLifecycle(t, req, dir, stub, false, true)
	assertLifecycle(t, exp, stub, got, dir, req.CacheFileName)
	if *exp.FetchedAt != req.FetchedAtFetched {
		t.Fatalf("expected fetchedAt to equal fetchedAtFetched %q", req.FetchedAtFetched)
	}
}

func TestLifecycleOffline(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	exp := loadLifecycleExpected(t, "offline")
	dir := t.TempDir()
	installCache(t, dir, req.CacheFileName, "cache-stale.json")
	stub := &recordingFetch{payload: sourcePayload(t)}
	got := runLifecycle(t, req, dir, stub, true, false)
	assertLifecycle(t, exp, stub, got, dir, req.CacheFileName)
	if *exp.FetchedAt != req.FetchedAtStale {
		t.Fatalf("expected fetchedAt to equal fetchedAtStale %q", req.FetchedAtStale)
	}
}

func TestLifecycleOfflineNoCache(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	exp := loadLifecycleExpected(t, "offline-no-cache")
	dir := t.TempDir()
	stub := &recordingFetch{payload: sourcePayload(t)}
	got := runLifecycle(t, req, dir, stub, true, false)
	assertLifecycle(t, exp, stub, got, dir, req.CacheFileName)
	if got.Pricing.Priced {
		t.Fatalf("offline-no-cache should degrade to a miss")
	}
}

func TestLifecycleStaleFallback(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	exp := loadLifecycleExpected(t, "stale-fallback")
	dir := t.TempDir()
	installCache(t, dir, req.CacheFileName, "cache-stale.json")
	stub := &recordingFetch{payload: sourcePayload(t), throws: true}
	got := runLifecycle(t, req, dir, stub, false, false)
	assertLifecycle(t, exp, stub, got, dir, req.CacheFileName)
	if *exp.FetchedAt != req.FetchedAtStale {
		t.Fatalf("expected fetchedAt to equal fetchedAtStale %q", req.FetchedAtStale)
	}
}

// --- the three hardening behaviors ------------------------------------------

// TestLifecycleFetchedEmpty: a fetch that probes to zero prices is treated like
// a fetch failure: the stale catalog is served, the cache file is NOT
// overwritten, and the rollup still prices.
func TestLifecycleFetchedEmpty(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	exp := loadLifecycleExpected(t, "fetched-empty")
	dir := t.TempDir()
	installCache(t, dir, req.CacheFileName, "cache-stale.json")
	var empty json.RawMessage
	loadLifecycleFixture(t, &empty, "source-openrouter-empty.json")
	stub := &recordingFetch{payload: empty}
	got := runLifecycle(t, req, dir, stub, false, false)
	assertLifecycle(t, exp, stub, got, dir, req.CacheFileName)
	if *exp.FetchedAt != req.FetchedAtStale {
		t.Fatalf("expected fetchedAt to equal fetchedAtStale %q", req.FetchedAtStale)
	}
}

// TestLifecycleFetchedInvalidJSON: a 2xx body that is not valid JSON is also
// treated like a fetch failure, never poisoning the cache (same stale-fallback
// rule as fetched-empty).
func TestLifecycleFetchedInvalidJSON(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	exp := loadLifecycleExpected(t, "fetched-empty")
	dir := t.TempDir()
	installCache(t, dir, req.CacheFileName, "cache-stale.json")
	stub := &recordingFetch{payload: []byte("<html>error</html>")}
	got := runLifecycle(t, req, dir, stub, false, false)
	assertLifecycle(t, exp, stub, got, dir, req.CacheFileName)
	if *exp.FetchedAt != req.FetchedAtStale {
		t.Fatalf("expected fetchedAt to equal fetchedAtStale %q", req.FetchedAtStale)
	}
}

// TestLifecycleCachedEmpty: a fresh cache that probes to zero prices is
// unusable, same as no cache: a fetch is issued and the cache is rewritten.
func TestLifecycleCachedEmpty(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	exp := loadLifecycleExpected(t, "cached-empty")
	dir := t.TempDir()
	installCache(t, dir, req.CacheFileName, "cache-fresh-empty.json")
	stub := &recordingFetch{payload: sourcePayload(t)}
	got := runLifecycle(t, req, dir, stub, false, false)
	assertLifecycle(t, exp, stub, got, dir, req.CacheFileName)
	if *exp.FetchedAt != req.FetchedAtFetched {
		t.Fatalf("expected fetchedAt to equal fetchedAtFetched %q", req.FetchedAtFetched)
	}
}

type lifecycleReloadExpected struct {
	Behavior        string  `json:"behavior"`
	FetchesTotal    int     `json:"fetchesTotal"`
	FetchedAtFirst  string  `json:"fetchedAtFirst"`
	FetchedAtSecond string  `json:"fetchedAtSecond"`
	Priced          bool    `json:"priced"`
	PricedUSD       float64 `json:"pricedUsd"`
}

// TestLifecycleLiveReload: one long-lived instance, empty cacheDir. Query 1 at
// now fetches and prices; the clock then advances past the TTL window, so query
// 2 on the SAME instance reloads (the disk cache is now stale) and issues a
// second fetch. Total fetches = fetchesTotal (2); each query prices.
func TestLifecycleLiveReload(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	var exp lifecycleReloadExpected
	loadLifecycleFixture(t, &exp, "expected", "live-reload.json")
	if exp.Behavior != "live-reload" {
		t.Fatalf("behavior mismatch: %q", exp.Behavior)
	}
	dir := t.TempDir()

	// a mutable clock the test advances between the two queries
	first, err := time.Parse(time.RFC3339Nano, req.Now)
	if err != nil {
		t.Fatalf("parse now: %v", err)
	}
	second, err := time.Parse(time.RFC3339Nano, req.NowSecond)
	if err != nil {
		t.Fatalf("parse nowSecond: %v", err)
	}
	current := first
	clock := func() time.Time { return current }

	stub := &recordingFetch{payload: sourcePayload(t)}
	make := sourceFactory(t, req.Source)
	pricing, err := NewPricing(PricingOptions{
		Mode:     req.Options.Mode,
		Sources:  []PricingSource{make(sourceURL)},
		CacheDir: dir,
		TTLMs:    req.TTLMs,
		Fetch:    stub.fetch,
		Now:      clock,
	})
	if err != nil {
		t.Fatalf("NewPricing: %v", err)
	}
	defer pricing.Close()

	priced1, err := pricing.PriceRollups([]RollupBucket{req.Rollup})
	if err != nil {
		t.Fatalf("PriceRollups 1: %v", err)
	}
	// both queries' priced state and USD are driven from the committed expected
	// fields (design section 7's every-field requirement)
	if priced1[0].Pricing.Priced != exp.Priced {
		t.Fatalf("query 1 priced = %v, want %v", priced1[0].Pricing.Priced, exp.Priced)
	}
	usd1 := 0.0
	if priced1[0].Pricing.USD != nil {
		usd1 = *priced1[0].Pricing.USD
	}
	if !closeEnough(usd1, exp.PricedUSD) {
		t.Fatalf("query 1 usd = %v, want %v", usd1, exp.PricedUSD)
	}
	if stamp := currentFetchedAt(t, dir, req.CacheFileName); stamp == nil || *stamp != exp.FetchedAtFirst {
		t.Fatalf("query 1 fetchedAt = %v, want %q", stamp, exp.FetchedAtFirst)
	}

	current = second
	priced2, err := pricing.PriceRollups([]RollupBucket{req.Rollup})
	if err != nil {
		t.Fatalf("PriceRollups 2: %v", err)
	}
	if priced2[0].Pricing.Priced != exp.Priced {
		t.Fatalf("query 2 priced = %v, want %v", priced2[0].Pricing.Priced, exp.Priced)
	}
	usd2 := 0.0
	if priced2[0].Pricing.USD != nil {
		usd2 = *priced2[0].Pricing.USD
	}
	if !closeEnough(usd2, exp.PricedUSD) {
		t.Fatalf("query 2 usd = %v, want %v", usd2, exp.PricedUSD)
	}
	if stub.calls != exp.FetchesTotal {
		t.Fatalf("total fetches = %d, want %d", stub.calls, exp.FetchesTotal)
	}
	if stamp := currentFetchedAt(t, dir, req.CacheFileName); stamp == nil || *stamp != exp.FetchedAtSecond {
		t.Fatalf("query 2 fetchedAt = %v, want %q", stamp, exp.FetchedAtSecond)
	}
}

// scriptedSource is a PricingSource whose Load throws on the load indices in
// failOn (1-based) and otherwise returns the given catalog. It counts its Load
// calls so a test can prove a failed reload is retried rather than memoized.
type scriptedSource struct {
	catalog *Catalog
	failOn  map[int]bool
	calls   int
}

func (s *scriptedSource) Name() string { return "scripted" }

func (s *scriptedSource) Load(SourceContext) (*Catalog, error) {
	s.calls++
	if s.failOn[s.calls] {
		return nil, errNetworkDown
	}
	c := *s.catalog
	return &c, nil
}

// TestLifecycleFailedReloadRetries proves an expired reload that FAILS is not
// memoized: load 1 succeeds, the clock advances past the TTL, reload attempt 1
// throws, and a later query retries the reload and succeeds. Expiry stays in
// force across the failure. The source Load counts prove the retry.
func TestLifecycleFailedReloadRetries(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")

	first, err := time.Parse(time.RFC3339Nano, req.Now)
	if err != nil {
		t.Fatalf("parse now: %v", err)
	}
	second, err := time.Parse(time.RFC3339Nano, req.NowSecond)
	if err != nil {
		t.Fatalf("parse nowSecond: %v", err)
	}
	current := first
	clock := func() time.Time { return current }

	// the second Load (the first reload attempt) throws; loads 1 and 3 succeed
	src := &scriptedSource{
		catalog: &Catalog{Source: "litellm", FetchedAt: req.Now, Format: "litellm", Payload: readCatalogPayload(t, "litellm")},
		failOn:  map[int]bool{2: true},
	}
	pricing, err := NewPricing(PricingOptions{
		Mode:     ModeCalculate,
		Sources:  []PricingSource{src},
		CacheDir: t.TempDir(),
		TTLMs:    req.TTLMs,
		Now:      clock,
	})
	if err != nil {
		t.Fatalf("NewPricing: %v", err)
	}
	defer pricing.Close()
	if src.calls != 1 {
		t.Fatalf("after load 1 source loads = %d, want 1", src.calls)
	}

	// advance past the TTL so the next query triggers a reload
	current = second

	// reload attempt 1: the source throws, so the load errors and is not memoized
	if _, err := pricing.LookupModel("gpt-5"); err == nil {
		t.Fatalf("expected the failed reload to surface an error")
	}
	if src.calls != 2 {
		t.Fatalf("after failed reload source loads = %d, want 2", src.calls)
	}

	// the failure is not memoized: a later query retries the reload and succeeds
	hit, err := pricing.LookupModel("gpt-5")
	if err != nil {
		t.Fatalf("retry LookupModel: %v", err)
	}
	if !hit.Priced {
		t.Fatalf("retry should be a hit")
	}
	if src.calls != 3 {
		t.Fatalf("after retry source loads = %d, want 3 (the retry re-loaded)", src.calls)
	}
}

// --- table-driven per-source fetch ----------------------------------------

// TestLifecyclePerSource drives each built-in source over its canonical
// source-*.json payload: a cold fetch writes the exact cache filename, the
// catalog provenance carries the exact source name, and the shared rollup
// prices to 2.25 through that source's core parser.
func TestLifecyclePerSource(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	cases := []struct {
		fixture   string
		make      func(url string) PricingSource
		source    string
		cacheName string
	}{
		{"source-openrouter.json", OpenRouterSource, "openrouter", "pricing-openrouter.json"},
		{"source-litellm.json", LiteLLMSource, "litellm", "pricing-litellm.json"},
		{"source-modelsdev.json", ModelsDevSource, "models-dev", "pricing-models-dev.json"},
	}
	for _, tc := range cases {
		t.Run(tc.source, func(t *testing.T) {
			dir := t.TempDir()
			var payload json.RawMessage
			loadLifecycleFixture(t, &payload, tc.fixture)
			stub := &recordingFetch{payload: payload}
			pricing, err := NewPricing(PricingOptions{
				Mode:     req.Options.Mode,
				Sources:  []PricingSource{tc.make("https://" + tc.source + ".test/api")},
				CacheDir: dir,
				TTLMs:    req.TTLMs,
				Fetch:    stub.fetch,
				Now:      fixedClock(t, req.Now),
			})
			if err != nil {
				t.Fatalf("NewPricing: %v", err)
			}
			defer pricing.Close()

			if stub.calls != 1 {
				t.Fatalf("%s: fetch calls = %d, want 1", tc.source, stub.calls)
			}
			infos, err := pricing.Catalogs()
			if err != nil {
				t.Fatalf("Catalogs: %v", err)
			}
			found := false
			for _, info := range infos {
				if info.Source == tc.source {
					found = true
				}
			}
			if !found {
				t.Fatalf("%s: catalog provenance missing source %q: %+v", tc.source, tc.source, infos)
			}
			// exact cache filename written for this source
			if _, err := os.Stat(filepath.Join(dir, tc.cacheName)); err != nil {
				t.Fatalf("%s: expected cache file %q: %v", tc.source, tc.cacheName, err)
			}
			priced, err := pricing.PriceRollups([]RollupBucket{req.Rollup})
			if err != nil {
				t.Fatalf("PriceRollups: %v", err)
			}
			if !priced[0].Pricing.Priced {
				t.Fatalf("%s: should price", tc.source)
			}
			usd := 0.0
			if priced[0].Pricing.USD != nil {
				usd = *priced[0].Pricing.USD
			}
			if !closeEnough(usd, 2.25) {
				t.Fatalf("%s: usd = %v, want 2.25", tc.source, usd)
			}
		})
	}
}

// --- concurrent close/dispose race protection-----------------------------

// TestConcurrentCloseIsExactlyOnce spins up many concurrent Close calls plus a
// concurrent query on the same handle; the mutex must make the free
// exactly-once and prevent a call from overlapping the free (no double-free,
// no use-after-free). Run under -race to exercise the discipline.
func TestConcurrentCloseIsExactlyOnce(t *testing.T) {
	const pinned = "2026-08-01T00:00:00.000Z"
	pricing, err := NewPricing(PricingOptions{
		Mode:    ModeCalculate,
		Sources: []PricingSource{},
		Catalogs: []Catalog{
			{Source: "litellm", FetchedAt: pinned, Format: "litellm", Payload: readCatalogPayload(t, "litellm")},
		},
	})
	if err != nil {
		t.Fatalf("NewPricing: %v", err)
	}

	var wg sync.WaitGroup
	for i := 0; i < 16; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			pricing.Close()
		}()
	}
	wg.Add(1)
	go func() {
		defer wg.Done()
		// a query racing the closes: it either succeeds (before free) or
		// errors on a nil handle, but never double-frees or reads freed memory
		_, _ = pricing.LookupModel("gpt-5")
	}()
	wg.Wait()
	// a final Close is still safe (idempotent)
	pricing.Close()
}

// TestQueryAfterCloseReturnsClosed asserts Close is terminal: a query after
// Close returns ErrClosed and does NOT resurrect the handle by fetching or
// rebuilding. The fetch count proves no reload was attempted after Close.
func TestQueryAfterCloseReturnsClosed(t *testing.T) {
	var req lifecycleRequest
	loadLifecycleFixture(t, &req, "request.json")
	dir := t.TempDir()
	stub := &recordingFetch{payload: sourcePayload(t)}
	make := sourceFactory(t, req.Source)
	pricing, err := NewPricing(PricingOptions{
		Mode:     req.Options.Mode,
		Sources:  []PricingSource{make(sourceURL)},
		CacheDir: dir,
		TTLMs:    req.TTLMs,
		Fetch:    stub.fetch,
		Now:      fixedClock(t, req.Now),
	})
	if err != nil {
		t.Fatalf("NewPricing: %v", err)
	}
	if stub.calls != 1 {
		t.Fatalf("initial load fetches = %d, want 1", stub.calls)
	}

	pricing.Close()

	// a query after Close must fail with ErrClosed and never fetch or rebuild
	if _, err := pricing.LookupModel("gpt-5"); !errors.Is(err, ErrClosed) {
		t.Fatalf("LookupModel after Close = %v, want ErrClosed", err)
	}
	if _, err := pricing.PriceRollups([]RollupBucket{req.Rollup}); !errors.Is(err, ErrClosed) {
		t.Fatalf("PriceRollups after Close = %v, want ErrClosed", err)
	}
	if _, err := pricing.Catalogs(); !errors.Is(err, ErrClosed) {
		t.Fatalf("Catalogs after Close = %v, want ErrClosed", err)
	}
	if stub.calls != 1 {
		t.Fatalf("fetches after Close = %d, want 1 (no resurrection)", stub.calls)
	}
}

// --- rollup gold equality + lookup hit/miss ---------------------------------

func hermeticPricing(t *testing.T) *Pricing {
	t.Helper()
	const pinned = "2026-08-01T00:00:00.000Z"
	pricing, err := NewPricing(PricingOptions{
		Mode:    ModeCalculate,
		Sources: []PricingSource{},
		Catalogs: []Catalog{
			{Source: "openrouter", FetchedAt: pinned, Format: "openrouter", Payload: readCatalogPayload(t, "openrouter")},
			{Source: "litellm", FetchedAt: pinned, Format: "litellm", Payload: readCatalogPayload(t, "litellm")},
		},
	})
	if err != nil {
		t.Fatalf("NewPricing: %v", err)
	}
	return pricing
}

// TestPriceRollupsMatchesGold is raw-JSON-tree gold equality,
// not a projection through facade types: the expected gold rollups are parsed
// as raw JSON objects and never decoded through PricedRollup. Input is a raw
// clone of each expected object with ONLY the pricing field deleted, decoded
// into RollupBucket for the PUBLIC PriceRollups call. The actual return is
// marshalled back to a generic JSON tree and the COMPLETE tree is compared
// against the untouched expected array (canonicalizing key order and number
// representation only). This is the same approach the C# test takes.
func TestPriceRollupsMatchesGold(t *testing.T) {
	pricing := hermeticPricing(t)
	defer pricing.Close()

	goldPath := filepath.Join(repoRoot(t), "golden", "pricing", "basic", "expected-priced-rollup.json")
	goldBytes, err := os.ReadFile(goldPath)
	if err != nil {
		t.Fatalf("read gold: %v", err)
	}
	var gold struct {
		Rollups []json.RawMessage `json:"rollups"`
	}
	if err := json.Unmarshal(goldBytes, &gold); err != nil {
		t.Fatalf("decode gold: %v", err)
	}

	// input: a raw copy of each expected rollup with ONLY the pricing field
	// removed, decoded into the facade RollupBucket for the public API call
	input := make([]RollupBucket, len(gold.Rollups))
	for i, raw := range gold.Rollups {
		var obj map[string]json.RawMessage
		if err := json.Unmarshal(raw, &obj); err != nil {
			t.Fatalf("decode gold rollup %d: %v", i, err)
		}
		delete(obj, "pricing")
		clone, err := json.Marshal(obj)
		if err != nil {
			t.Fatalf("marshal input rollup %d: %v", i, err)
		}
		if err := json.Unmarshal(clone, &input[i]); err != nil {
			t.Fatalf("decode input rollup %d: %v", i, err)
		}
	}

	priced, err := pricing.PriceRollups(input)
	if err != nil {
		t.Fatalf("PriceRollups: %v", err)
	}

	// encode the actual returned values back to a generic JSON tree and compare
	// the complete tree against the untouched expected array
	actualBytes, err := json.Marshal(priced)
	if err != nil {
		t.Fatalf("marshal priced: %v", err)
	}
	var actualTree any
	if err := json.Unmarshal(actualBytes, &actualTree); err != nil {
		t.Fatalf("decode actual tree: %v", err)
	}
	var expectedTree any
	if err := json.Unmarshal(goldBytes, &struct {
		Rollups *any `json:"rollups"`
	}{Rollups: &expectedTree}); err != nil {
		t.Fatalf("decode expected tree: %v", err)
	}

	if !reflect.DeepEqual(actualTree, expectedTree) {
		t.Fatalf("priceRollups mismatch:\n got: %s\nwant: %s", actualBytes, mustJSON(t, expectedTree))
	}
}

func TestLookupModelHitAndMiss(t *testing.T) {
	pricing := hermeticPricing(t)
	defer pricing.Close()

	hit, err := pricing.LookupModel("gpt-5")
	if err != nil {
		t.Fatalf("LookupModel(gpt-5): %v", err)
	}
	if !hit.Priced {
		t.Fatalf("gpt-5 should be a hit: %+v", hit)
	}
	if hit.Source != "litellm" {
		t.Fatalf("gpt-5 source = %q, want litellm", hit.Source)
	}

	miss, err := pricing.LookupModel("totally-unknown-model-9000")
	if err != nil {
		t.Fatalf("LookupModel(miss): %v", err)
	}
	if miss.Priced {
		t.Fatalf("unknown model should be a miss: %+v", miss)
	}
}

type constantsSource struct {
	Name          string `json:"name"`
	Format        string `json:"format"`
	URL           string `json:"url"`
	CacheFileName string `json:"cacheFileName"`
}

type lifecycleConstants struct {
	Sources              []constantsSource `json:"sources"`
	PriorityOrder        []string          `json:"priorityOrder"`
	DefaultTTLMs         int64             `json:"defaultTtlMs"`
	FetchTimeoutMs       int64             `json:"fetchTimeoutMs"`
	CacheFileNamePattern string            `json:"cacheFileNamePattern"`
}

// The shared golden constants contract (constants.json): the facade's own source
// names, formats, URLs, priority order, TTL default, fetch timeout, and cache
// filenames must equal the golden values so no facade's literals drift.
func TestLifecycleConstantsMatchGolden(t *testing.T) {
	var constants lifecycleConstants
	loadLifecycleFixture(t, &constants, "constants.json")

	urlByName := map[string]string{
		"openrouter": OpenRouterURL,
		"litellm":    LiteLLMURL,
		"models-dev": ModelsDevURL,
	}

	// the default built-in sources in the reference priority order
	builtin := builtinSources()
	if len(builtin) != len(constants.PriorityOrder) {
		t.Fatalf("builtin source count %d, want %d", len(builtin), len(constants.PriorityOrder))
	}
	byName := map[string]*cachedSource{}
	for i, src := range builtin {
		cs, ok := src.(*cachedSource)
		if !ok {
			t.Fatalf("builtin source %d is not *cachedSource", i)
		}
		if cs.name != constants.PriorityOrder[i] {
			t.Fatalf("priority[%d] = %q, want %q", i, cs.name, constants.PriorityOrder[i])
		}
		byName[cs.name] = cs
	}
	for i, spec := range constants.Sources {
		if spec.Name != constants.PriorityOrder[i] {
			t.Fatalf("sources[%d].name = %q, want priorityOrder %q", i, spec.Name, constants.PriorityOrder[i])
		}
	}

	for _, spec := range constants.Sources {
		cs, ok := byName[spec.Name]
		if !ok {
			t.Fatalf("no builtin source named %q", spec.Name)
		}
		if cs.format != spec.Format {
			t.Fatalf("%s format = %q, want %q", spec.Name, cs.format, spec.Format)
		}
		if cs.url != spec.URL {
			t.Fatalf("%s url = %q, want %q", spec.Name, cs.url, spec.URL)
		}
		if urlByName[spec.Name] != spec.URL {
			t.Fatalf("%s const URL = %q, want %q", spec.Name, urlByName[spec.Name], spec.URL)
		}
		wantCache := strings.Replace(constants.CacheFileNamePattern, "{name}", spec.Name, 1)
		if spec.CacheFileName != wantCache {
			t.Fatalf("%s cacheFileName = %q, want %q", spec.Name, spec.CacheFileName, wantCache)
		}
		// compare the production filename formula (cacheFileName), not just the
		// fixture pattern, so the real formula cannot drift unseen
		if got := cacheFileName(spec.Name); got != spec.CacheFileName {
			t.Fatalf("%s cacheFileName formula = %q, want %q", spec.Name, got, spec.CacheFileName)
		}
	}

	if DefaultTTLMs != constants.DefaultTTLMs {
		t.Fatalf("DefaultTTLMs = %d, want %d", DefaultTTLMs, constants.DefaultTTLMs)
	}
	// the facade keeps the fetch timeout as a time.Duration; the golden value is in ms
	if got := fetchTimeout.Milliseconds(); got != constants.FetchTimeoutMs {
		t.Fatalf("fetchTimeout = %d ms, want %d ms", got, constants.FetchTimeoutMs)
	}
}
