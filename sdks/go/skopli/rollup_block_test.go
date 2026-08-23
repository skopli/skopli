// Billing-block windowing conformance, driven by the SHARED fixtures in
// golden/rollup-block/. Each case rolls one synthetic event per timestamp into
// blocks of the case's width in the case's zone and asserts the resulting blocks
// equal the shared gold, exercising the same native core over the C ABI that the
// Rust and other-language suites use.
package skopli

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

type blockBucket struct {
	Key    string `json:"key"`
	Events uint64 `json:"events"`
}

type blockCase struct {
	Name       string        `json:"name"`
	TZ         string        `json:"tz"`
	BlockMs    int64         `json:"blockMs"`
	Timestamps []string      `json:"timestamps"`
	Expected   []blockBucket `json:"expected"`
}

func TestRollupBlockConformance(t *testing.T) {
	p := filepath.Join(repoRoot(t), "golden", "rollup-block", "cases.json")
	b, err := os.ReadFile(p)
	if err != nil {
		t.Fatalf("read rollup-block cases: %v", err)
	}
	var cases []blockCase
	if err := json.Unmarshal(b, &cases); err != nil {
		t.Fatalf("decode rollup-block cases: %v", err)
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
			rollups, err := Rollup(events, RollupOptions{By: ByBlock, TZ: kase.TZ, BlockMs: kase.BlockMs})
			if err != nil {
				t.Fatalf("Rollup: %v", err)
			}
			if len(rollups) != len(kase.Expected) {
				t.Fatalf("block count = %d, want %d: %+v", len(rollups), len(kase.Expected), rollups)
			}
			for i, want := range kase.Expected {
				if rollups[i].Key != want.Key || rollups[i].Events != want.Events {
					t.Fatalf("block %d = {%s, %d}, want {%s, %d}",
						i, rollups[i].Key, rollups[i].Events, want.Key, want.Events)
				}
			}
		})
	}
}

// Omitting BlockMs (leaving it zero, which the omitempty tag drops from the
// wire) must apply the five-hour default: two events 3h41m apart join one block
// anchored to 09:00, and an event past five hours opens a new block. Guards the
// facade's absent-optional serialization, not the fixture's explicit width.
func TestRollupBlockOmittedWidthUsesDefault(t *testing.T) {
	mk := func(timestamps ...string) []UsageEvent {
		events := make([]UsageEvent, len(timestamps))
		for i, ts := range timestamps {
			events[i] = UsageEvent{Harness: "h", Timestamp: ts, SessionID: "s", MessageID: "m", Model: "m"}
		}
		return events
	}
	assertBlocks := func(got []RollupBucket, want []blockBucket) {
		t.Helper()
		if len(got) != len(want) {
			t.Fatalf("block count = %d, want %d: %+v", len(got), len(want), got)
		}
		for i, w := range want {
			if got[i].Key != w.Key || got[i].Events != w.Events {
				t.Fatalf("block %d = {%s, %d}, want {%s, %d}", i, got[i].Key, got[i].Events, w.Key, w.Events)
			}
		}
	}

	joined, err := Rollup(mk("2026-01-01T09:17:00.000Z", "2026-01-01T13:00:00.000Z"), RollupOptions{By: ByBlock, TZ: "UTC"})
	if err != nil {
		t.Fatalf("Rollup joined: %v", err)
	}
	assertBlocks(joined, []blockBucket{{Key: "2026-01-01T09:00:00.000Z", Events: 2}})

	split, err := Rollup(mk("2026-01-01T09:00:00.000Z", "2026-01-01T14:30:00.000Z"), RollupOptions{By: ByBlock, TZ: "UTC"})
	if err != nil {
		t.Fatalf("Rollup split: %v", err)
	}
	assertBlocks(split, []blockBucket{
		{Key: "2026-01-01T09:00:00.000Z", Events: 1},
		{Key: "2026-01-01T14:00:00.000Z", Events: 1},
	})
}
