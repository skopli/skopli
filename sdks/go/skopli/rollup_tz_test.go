// Timezone day-bucketing conformance, driven by the SHARED fixtures in
// golden/rollup-tz/. Each case rolls one synthetic event per timestamp up by
// day in the case's zone and asserts the resulting buckets equal the shared
// gold, exercising the same native core over the C ABI that the Rust and
// other-language suites use.
package skopli

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

type tzBucket struct {
	Key    string `json:"key"`
	Events uint64 `json:"events"`
}

type tzCase struct {
	Name       string     `json:"name"`
	TZ         string     `json:"tz"`
	Timestamps []string   `json:"timestamps"`
	Expected   []tzBucket `json:"expected"`
}

func TestRollupTzConformance(t *testing.T) {
	p := filepath.Join(repoRoot(t), "golden", "rollup-tz", "cases.json")
	b, err := os.ReadFile(p)
	if err != nil {
		t.Fatalf("read rollup-tz cases: %v", err)
	}
	var cases []tzCase
	if err := json.Unmarshal(b, &cases); err != nil {
		t.Fatalf("decode rollup-tz cases: %v", err)
	}
	for _, kase := range cases {
		t.Run(kase.Name, func(t *testing.T) {
			events := make([]UsageEvent, len(kase.Timestamps))
			for i, ts := range kase.Timestamps {
				events[i] = UsageEvent{
					Harness:   "h",
					Timestamp: ts,
					SessionID: "s",
					MessageID: "m",
					Model:     "m",
				}
			}
			rollups, err := Rollup(events, RollupOptions{By: ByDay, TZ: kase.TZ})
			if err != nil {
				t.Fatalf("Rollup: %v", err)
			}
			if len(rollups) != len(kase.Expected) {
				t.Fatalf("bucket count = %d, want %d: %+v", len(rollups), len(kase.Expected), rollups)
			}
			for i, want := range kase.Expected {
				if rollups[i].Key != want.Key || rollups[i].Events != want.Events {
					t.Fatalf("bucket %d = {%s, %d}, want {%s, %d}",
						i, rollups[i].Key, rollups[i].Events, want.Key, want.Events)
				}
			}
		})
	}
}
