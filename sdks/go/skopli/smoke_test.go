// Smoke test driving the Go facade over the C ABI end to end: read -> rollup ->
// price against the committed golden case, asserting structural parity with the
// shared conformance gold (the same golden/claude/basic + pricing/catalogs data
// the Rust capi smoke test uses). Also verifies the errors.Is sentinel mapping
// for an invalid-argument and a catalog error.
//
// The test needs the GNU-target cdylib on the loader path at run time; see the
// package README for the CGO/PATH mechanics.
package skopli

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"testing"
)

// repoRoot walks up from this test file to the workspace root (four parents:
// smoke_test.go -> skopli -> go -> sdks -> <root>).
func repoRoot(t *testing.T) string {
	t.Helper()
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("cannot resolve caller file")
	}
	// file = <root>/sdks/go/skopli/smoke_test.go
	return filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
}

func claudeInputDir(t *testing.T) string {
	return filepath.Join(repoRoot(t), "golden", "claude", "basic", "input")
}

func claudeReadOptions(t *testing.T) ReadUsageOptions {
	return ReadUsageOptions{
		Options: Options{
			Home: "/nonexistent",
			Env:  map[string]string{"CLAUDE_CONFIG_DIR": claudeInputDir(t)},
		},
		Harnesses: []Harness{HarnessClaude},
		TZ:        "UTC",
	}
}

// --- meta -------------------------------------------------------------------

func TestMeta(t *testing.T) {
	if got := ABIVersion(); got != 1 {
		t.Fatalf("ABIVersion = %d, want 1", got)
	}
	if got := SchemaVersion(); got != 1 {
		t.Fatalf("SchemaVersion = %d, want 1", got)
	}
	if Version() == "" {
		t.Fatal("Version is empty")
	}
	dir, err := DefaultCacheDir()
	if err != nil {
		t.Fatalf("DefaultCacheDir: %v", err)
	}
	if dir == "" {
		t.Fatal("DefaultCacheDir is empty")
	}
}

// --- harness enum parity ----------------------------------------------------

// TestHarnessEnumCoversRegistry asserts KnownHarnesses covers every id in the
// shared registry-id fixture, so a reader added to the core cannot silently
// fall out of the Go surface.
func TestHarnessEnumCoversRegistry(t *testing.T) {
	path := filepath.Join(repoRoot(t), "golden", "registry", "ids.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read registry ids fixture: %v", err)
	}
	var fixture struct {
		IDs []string `json:"ids"`
	}
	if err := json.Unmarshal(raw, &fixture); err != nil {
		t.Fatalf("parse registry ids fixture: %v", err)
	}
	known := make(map[Harness]bool, len(KnownHarnesses))
	for _, h := range KnownHarnesses {
		known[h] = true
	}
	for _, id := range fixture.IDs {
		if !known[Harness(id)] {
			t.Errorf("KnownHarnesses missing registry id %q", id)
		}
	}
	registered := make(map[string]bool, len(fixture.IDs))
	for _, id := range fixture.IDs {
		registered[id] = true
	}
	for _, h := range KnownHarnesses {
		if !registered[string(h)] {
			t.Errorf("KnownHarnesses has id %q the registry does not", h)
		}
	}
}

// --- detect -----------------------------------------------------------------

func TestDetectFindsClaude(t *testing.T) {
	opts := claudeReadOptions(t).Options
	detection, err := DetectHarnesses(&opts)
	if err != nil {
		t.Fatalf("DetectHarnesses: %v", err)
	}
	found := false
	for _, h := range detection.Supported {
		if h == HarnessClaude {
			found = true
		}
	}
	if !found {
		t.Fatalf("claude not in supported: %+v", detection)
	}
	if len(detection.Unsupported) != 0 {
		t.Fatalf("unsupported should be empty over this ABI: %+v", detection.Unsupported)
	}
}

// --- cost_usd ---------------------------------------------------------------

func TestCostUSDFlat(t *testing.T) {
	tokens := TokenCounts{Input: 1000, Output: 500}
	price := ModelPrice{Input: 1.25, Output: 10.0}
	got, err := CostUSD(tokens, price)
	if err != nil {
		t.Fatalf("CostUSD: %v", err)
	}
	want := (1000.0*1.25 + 500.0*10.0) / 1_000_000.0
	if got != want {
		t.Fatalf("CostUSD = %v, want %v", got, want)
	}
}

// --- read -> rollup round trip vs gold --------------------------------------

func TestReadRollupRoundtripMatchesGold(t *testing.T) {
	result, err := ReadUsage(claudeReadOptions(t))
	if err != nil {
		t.Fatalf("ReadUsage: %v", err)
	}
	if len(result.Events) == 0 {
		t.Fatal("read produced no events")
	}

	// Sort events the way the exporter/gold does before rolling up.
	events := append([]UsageEvent(nil), result.Events...)
	sort.SliceStable(events, func(i, j int) bool {
		a, b := events[i], events[j]
		if a.Timestamp != b.Timestamp {
			return a.Timestamp < b.Timestamp
		}
		if a.SessionID != b.SessionID {
			return a.SessionID < b.SessionID
		}
		if a.MessageID != b.MessageID {
			return a.MessageID < b.MessageID
		}
		return a.Model < b.Model
	})

	byModel, err := Rollup(events, RollupOptions{By: ByModel, TZ: "UTC"})
	if err != nil {
		t.Fatalf("Rollup: %v", err)
	}

	// Load the gold model lane and compare structurally (round-trip both sides
	// through the wire so pointer/omitempty encoding differences vanish).
	goldPath := filepath.Join(repoRoot(t), "golden", "claude", "basic", "expected-rollup.json")
	goldBytes, err := os.ReadFile(goldPath)
	if err != nil {
		t.Fatalf("read gold: %v", err)
	}
	var gold struct {
		By struct {
			Model []RollupBucket `json:"model"`
		} `json:"by"`
	}
	if err := json.Unmarshal(goldBytes, &gold); err != nil {
		t.Fatalf("decode gold: %v", err)
	}

	if !reflect.DeepEqual(normalize(t, byModel), normalize(t, gold.By.Model)) {
		t.Fatalf("rollup(by=model) mismatch:\n got: %s\nwant: %s",
			mustJSON(t, byModel), mustJSON(t, gold.By.Model))
	}
}

// --- pricing round trip -----------------------------------------------------

func readCatalogPayload(t *testing.T, name string) json.RawMessage {
	t.Helper()
	p := filepath.Join(repoRoot(t), "golden", "pricing", "catalogs", name+".json")
	b, err := os.ReadFile(p)
	if err != nil {
		t.Fatalf("read catalog %s: %v", name, err)
	}
	return json.RawMessage(b)
}

// pricingEvents mirrors the synthetic events from the pricing conformance case.
func pricingEvents() []UsageEvent {
	ev := func(mid, ts, model string, tk TokenCounts) UsageEvent {
		return UsageEvent{
			Harness:   HarnessOpencode,
			Timestamp: ts,
			SessionID: "s",
			MessageID: mid,
			Turn:      true,
			Model:     model,
			Tokens:    tk,
		}
	}
	u := func(v uint64) *uint64 { return &v }
	return []UsageEvent{
		ev("flat", "2026-08-01T00:00:00.000Z", "gpt-5",
			TokenCounts{Input: 1000, Output: 500}),
		ev("tiered", "2026-08-01T00:01:00.000Z", "claude-sonnet-4-5",
			TokenCounts{Input: 200000, Output: 10000, CacheRead: 20000, CacheWrite: 30000, CacheWrite1h: u(10000), Reasoning: 2000}),
		ev("base-1h", "2026-08-01T00:02:00.000Z", "claude-sonnet-4-5",
			TokenCounts{Input: 5000, Output: 1000, CacheRead: 2000, CacheWrite: 4000, CacheWrite1h: u(1500)}),
		ev("alias", "2026-08-01T00:03:00.000Z", "us.anthropic.claude-opus-4-6-20260115-v1:0",
			TokenCounts{Input: 800, Output: 200}),
		ev("miss", "2026-08-01T00:04:00.000Z", "totally-unknown-model-9000",
			TokenCounts{Input: 100, Output: 100}),
	}
}

func TestPricingRoundtrip(t *testing.T) {
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
	defer pricing.Close()

	infos, err := pricing.Catalogs()
	if err != nil {
		t.Fatalf("Catalogs: %v", err)
	}
	if len(infos) != 2 {
		t.Fatalf("want 2 catalogs, got %d: %+v", len(infos), infos)
	}
	if infos[0].Source != "openrouter" || infos[1].Source != "litellm" {
		t.Fatalf("catalog sources mismatch: %+v", infos)
	}
	if infos[0].Models == 0 {
		t.Fatalf("openrouter catalog has no models: %+v", infos[0])
	}

	priced, err := pricing.PriceEvents(pricingEvents(), RollupOptions{By: ByModel})
	if err != nil {
		t.Fatalf("PriceEvents: %v", err)
	}
	if len(priced) == 0 {
		t.Fatal("no priced groups")
	}

	var gpt5, miss *PricedRollup
	for i := range priced {
		switch priced[i].Key {
		case "gpt-5":
			gpt5 = &priced[i]
		case "totally-unknown-model-9000":
			miss = &priced[i]
		}
	}
	if gpt5 == nil {
		t.Fatal("gpt-5 group missing")
	}
	if !gpt5.Pricing.Priced {
		t.Fatalf("gpt-5 should be priced: %+v", gpt5.Pricing)
	}
	if gpt5.Pricing.USD == nil || *gpt5.Pricing.USD <= 0 {
		t.Fatalf("gpt-5 usd should be > 0: %+v", gpt5.Pricing)
	}
	if miss == nil {
		t.Fatal("miss group missing")
	}
	if miss.Pricing.Priced {
		t.Fatalf("unknown model should be a miss: %+v", miss.Pricing)
	}
}

// --- error sentinel mapping -------------------------------------------------

func TestErrInvalidArgument(t *testing.T) {
	_, err := ReadUsage(ReadUsageOptions{Since: "not-a-date"})
	if err == nil {
		t.Fatal("expected an error for a bad since date")
	}
	if !errors.Is(err, ErrInvalidArgument) {
		t.Fatalf("want ErrInvalidArgument, got %v", err)
	}
}

func TestErrCatalog(t *testing.T) {
	// A catalog with an unknown format triggers AG_STATUS_CATALOG.
	_, err := NewPricing(PricingOptions{
		Sources: []PricingSource{},
		Catalogs: []Catalog{
			{Source: "bogus", Format: "not-a-real-format", Payload: json.RawMessage(`{}`)},
		},
	})
	if err == nil {
		t.Fatal("expected a catalog error")
	}
	if !errors.Is(err, ErrCatalog) {
		t.Fatalf("want ErrCatalog, got %v", err)
	}
}

// --- helpers ----------------------------------------------------------------

// normalize round-trips a value through JSON so pointer vs value and omitempty
// encoding differences do not defeat DeepEqual.
func normalize(t *testing.T, v any) any {
	t.Helper()
	b := mustJSON(t, v)
	var out any
	if err := json.Unmarshal(b, &out); err != nil {
		t.Fatalf("normalize decode: %v", err)
	}
	return out
}

func mustJSON(t *testing.T, v any) []byte {
	t.Helper()
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	return b
}
