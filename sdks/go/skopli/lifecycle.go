package skopli

import (
	"encoding/json"
	"fmt"
	"io"
	"math/rand"
	"net/http"
	"os"
	"path/filepath"
	"time"
)

// Built-in source endpoints (mirroring src/pricing/sources.ts).
const (
	OpenRouterURL = "https://openrouter.ai/api/v1/models"
	LiteLLMURL    = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"
	ModelsDevURL  = "https://models.dev/api.json"

	fetchTimeout = 10 * time.Second
)

// httpFetch is the default net/http getter: a stalled endpoint must never hang
// an otherwise local run, so it carries a bounded timeout.
func httpFetch(url string) ([]byte, error) {
	client := &http.Client{Timeout: fetchTimeout}
	resp, err := client.Get(url)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("%s responded %d", url, resp.StatusCode)
	}
	return io.ReadAll(resp.Body)
}

// builtinSources returns the default priority-ordered market sources:
// OpenRouter > LiteLLM > models.dev.
func builtinSources() []PricingSource {
	return []PricingSource{
		OpenRouterSource(OpenRouterURL),
		LiteLLMSource(LiteLLMURL),
		ModelsDevSource(ModelsDevURL),
	}
}

// OpenRouterSource is the built-in OpenRouter market source.
func OpenRouterSource(url string) PricingSource {
	return &cachedSource{name: "openrouter", format: "openrouter", url: url}
}

// LiteLLMSource is the built-in LiteLLM market source.
func LiteLLMSource(url string) PricingSource {
	return &cachedSource{name: "litellm", format: "litellm", url: url}
}

// ModelsDevSource is the built-in models.dev market source.
func ModelsDevSource(url string) PricingSource {
	return &cachedSource{name: "models-dev", format: "modelsdev", url: url}
}

// cachedSource is a built-in market source: it fetches a raw payload, caches it
// byte-compatibly with the TS cache format, and serves the cache within TTL,
// offline, or as a stale fallback when a fetch fails. It never parses the
// payload; the raw bytes are handed to the core parser via the catalog
// format+payload path.
type cachedSource struct {
	name   string
	format string
	url    string
}

func (s *cachedSource) Name() string { return s.name }

// cacheFileName is the on-disk cache filename a built-in source reads and
// writes; the single production formula, used by Load and by the
// constants contract test in golden/pricing/lifecycle.
func cacheFileName(name string) string {
	return "pricing-" + name + ".json"
}

func (s *cachedSource) Load(ctx SourceContext) (*Catalog, error) {
	cacheName := cacheFileName(s.name)
	cached := loadCached(ctx.CacheDir, cacheName, ctx.TTLMs, ctx.Now())
	// a cache payload that probes to zero prices is unusable, same as no cache:
	// never serve or trust it before the core parser accepts it
	catalog := func() *Catalog {
		if cached == nil {
			return nil
		}
		c := &Catalog{Source: s.name, FetchedAt: cached.fetchedAt, Format: s.format, Payload: cached.payload}
		if !probeUsable(c) {
			return nil
		}
		return c
	}
	cacheFresh := cached != nil && !cached.stale && !ctx.Refresh
	if c := catalog(); c != nil && (cacheFresh || ctx.Offline) {
		return c, nil
	}
	if ctx.Offline {
		return nil, nil
	}
	fetch := ctx.Fetch
	if fetch == nil {
		return catalog(), nil
	}
	payload, err := fetch(s.url)
	if err != nil {
		// any-age stale fallback keeps pricing available when the network is not
		return catalog(), nil
	}
	// probe the fetched payload BEFORE writing the cache: an invalid-JSON or
	// zero-price body is treated as a fetch failure (serve the stale catalog if
	// one probed usable, else skip). Write-then-parse is the bug.
	candidate := &Catalog{Source: s.name, FetchedAt: isoUTC(ctx.Now()), Format: s.format, Payload: payload}
	if !probeUsable(candidate) {
		return catalog(), nil
	}
	fetchedAt, err := storeCached(ctx.CacheDir, cacheName, payload, ctx.Now())
	if err != nil {
		return nil, err
	}
	return &Catalog{Source: s.name, FetchedAt: fetchedAt, Format: s.format, Payload: payload}, nil
}

// probeUsable reports whether a raw catalog contributes at least one price
// through the core parser, without reimplementing the parser here. It builds a
// throwaway native pricing handle holding exactly the one raw catalog, reads
// the parsed model count from ag_pricing_catalog_info, and frees the handle.
// Any build error, or a zero model count, means the payload is unusable.
func probeUsable(catalog *Catalog) bool {
	native := nativePricingOptions{
		Mode:           string(ModeCalculate),
		Catalogs:       []Catalog{*catalog},
		BuiltinSources: false,
	}
	raw, err := json.Marshal(native)
	if err != nil {
		return false
	}
	handle, err := pricingNewRaw(raw)
	if err != nil {
		return false
	}
	defer handle.free()
	out, err := handle.catalogInfoRaw()
	if err != nil {
		return false
	}
	var infos []CatalogInfo
	if err := json.Unmarshal(out, &infos); err != nil {
		return false
	}
	for _, info := range infos {
		if info.Source == catalog.Source && info.Models > 0 {
			return true
		}
	}
	return false
}

// cachedPayload is a cache read result: the raw payload, its stamp, and whether
// its age exceeds the TTL.
type cachedPayload struct {
	fetchedAt string
	payload   json.RawMessage
	stale     bool
}

// cacheFile is the on-disk cache body, byte-compatible with src/pricing/cache.ts:
// {"fetchedAt": "<ISO-8601 UTC>", "payload": <raw source JSON>}.
type cacheFile struct {
	FetchedAt string          `json:"fetchedAt"`
	Payload   json.RawMessage `json:"payload"`
}

// loadCached reads a cache file, returning nil when it is absent, malformed, or
// carries an unparseable timestamp. stale is age > ttlMs (age == ttlMs fresh).
func loadCached(cacheDir, name string, ttlMs int64, now time.Time) *cachedPayload {
	raw, err := os.ReadFile(filepath.Join(cacheDir, name))
	if err != nil {
		return nil
	}
	var file cacheFile
	if err := json.Unmarshal(raw, &file); err != nil {
		return nil
	}
	if file.FetchedAt == "" || len(file.Payload) == 0 {
		return nil
	}
	fetched, err := time.Parse(time.RFC3339Nano, file.FetchedAt)
	if err != nil {
		return nil
	}
	age := now.Sub(fetched).Milliseconds()
	return &cachedPayload{fetchedAt: file.FetchedAt, payload: file.Payload, stale: age > ttlMs}
}

// storeCached writes a cache file atomically (temp sibling then os.Rename),
// stamped with now as an ISO-8601 UTC timestamp, and returns that stamp.
func storeCached(cacheDir, name string, payload []byte, now time.Time) (string, error) {
	fetchedAt := isoUTC(now)
	body, err := json.Marshal(cacheFile{FetchedAt: fetchedAt, Payload: json.RawMessage(payload)})
	if err != nil {
		return "", fmt.Errorf("%w: encoding cache file: %v", ErrInternal, err)
	}
	if err := os.MkdirAll(cacheDir, 0o755); err != nil {
		return "", fmt.Errorf("%w: creating cache dir: %v", ErrInternal, err)
	}
	tmp := filepath.Join(cacheDir, fmt.Sprintf("%s.%d.%d.tmp", name, os.Getpid(), rand.Int63()))
	if err := os.WriteFile(tmp, body, 0o644); err != nil {
		return "", fmt.Errorf("%w: writing cache file: %v", ErrInternal, err)
	}
	if err := os.Rename(tmp, filepath.Join(cacheDir, name)); err != nil {
		os.Remove(tmp)
		return "", fmt.Errorf("%w: renaming cache file: %v", ErrInternal, err)
	}
	return fetchedAt, nil
}

// isoUTC renders t as an ISO-8601 UTC timestamp with millisecond precision,
// matching the TS Date.toISOString() format the cache format pins.
func isoUTC(t time.Time) string {
	return t.UTC().Format("2006-01-02T15:04:05.000Z07:00")
}
