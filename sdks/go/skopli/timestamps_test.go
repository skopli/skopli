// Cache fetchedAt boundary conformance, driven by the SHARED fixtures in
// golden/pricing/timestamps/. The stamp is one wire contract read and written
// by seven facades; a stamp is read through this facade's own cache path
// (loadCached / storeCached) so Go and the native core agree on every boundary.
package skopli

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"
)

type tsReadCase struct {
	Name    string `json:"name"`
	Stamp   string `json:"stamp"`
	EpochMs *int64 `json:"epochMs"`
}

type tsCases struct {
	Read  []tsReadCase `json:"read"`
	Write []int64      `json:"write"`
}

func loadTimestampCases(t *testing.T) tsCases {
	t.Helper()
	p := filepath.Join(repoRoot(t), "golden", "pricing", "timestamps", "cases.json")
	b, err := os.ReadFile(p)
	if err != nil {
		t.Fatalf("read timestamps cases: %v", err)
	}
	var cases tsCases
	if err := json.Unmarshal(b, &cases); err != nil {
		t.Fatalf("decode timestamps cases: %v", err)
	}
	return cases
}

func writeStamp(t *testing.T, dir, name, stamp string) {
	t.Helper()
	body, err := json.Marshal(cacheFile{FetchedAt: stamp, Payload: json.RawMessage(`{}`)})
	if err != nil {
		t.Fatalf("marshal cache file: %v", err)
	}
	if err := os.WriteFile(filepath.Join(dir, name), body, 0o644); err != nil {
		t.Fatalf("write cache file: %v", err)
	}
}

func TestTimestampReadConformance(t *testing.T) {
	const name = "pricing-openrouter.json"
	for _, kase := range loadTimestampCases(t).Read {
		t.Run(kase.Name, func(t *testing.T) {
			dir := t.TempDir()
			writeStamp(t, dir, name, kase.Stamp)
			if kase.EpochMs == nil {
				if got := loadCached(dir, name, 0, time.UnixMilli(0)); got != nil {
					t.Fatalf("invalid stamp parsed to %+v, want nil", got)
				}
				return
			}
			// A 10 ms guard band brackets the parsed instant: gross misparses
			// (an ignored offset, a rejected pre-epoch) flip these assertions,
			// while sub-millisecond representation differences never do.
			ms := *kase.EpochMs
			if got := loadCached(dir, name, 0, time.UnixMilli(ms-10)); got == nil || got.stale {
				t.Fatalf("at now-10ms: got %+v, want fresh", got)
			}
			if got := loadCached(dir, name, 0, time.UnixMilli(ms+10)); got == nil || !got.stale {
				t.Fatalf("at now+10ms: got %+v, want stale", got)
			}
		})
	}
}

func TestTimestampWriteConformance(t *testing.T) {
	const name = "pricing-openrouter.json"
	for _, ms := range loadTimestampCases(t).Write {
		t.Run(time.UnixMilli(ms).UTC().Format(time.RFC3339Nano), func(t *testing.T) {
			dir := t.TempDir()
			stamp, err := storeCached(dir, name, []byte(`{}`), time.UnixMilli(ms))
			if err != nil {
				t.Fatalf("storeCached: %v", err)
			}
			if len(stamp) != 24 {
				t.Fatalf("stamp %q length = %d, want 24", stamp, len(stamp))
			}
			if stamp[len(stamp)-1] != 'Z' {
				t.Fatalf("stamp %q not Z-suffixed", stamp)
			}
			if got := loadCached(dir, name, 0, time.UnixMilli(ms-10)); got == nil || got.stale {
				t.Fatalf("at now-10ms: got %+v, want fresh", got)
			}
			if got := loadCached(dir, name, 0, time.UnixMilli(ms+10)); got == nil || !got.stale {
				t.Fatalf("at now+10ms: got %+v, want stale", got)
			}
		})
	}
}
