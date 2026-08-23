// Probe-contract conformance, driven by the SHARED fixtures in
// golden/pricing/probe/. Each case asserts probeUsable (the same internal
// function the source lifecycle uses) agrees with the shared usable/unusable
// gold, so a facade that drifts fails against shared gold.
package skopli

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

type probeCase struct {
	Name       string          `json:"name"`
	Format     string          `json:"format"`
	Payload    json.RawMessage `json:"payload"`
	PayloadRaw *string         `json:"payloadRaw"`
	Usable     bool            `json:"usable"`
}

func TestProbeConformance(t *testing.T) {
	p := filepath.Join(repoRoot(t), "golden", "pricing", "probe", "cases.json")
	b, err := os.ReadFile(p)
	if err != nil {
		t.Fatalf("read probe cases: %v", err)
	}
	var cases []probeCase
	if err := json.Unmarshal(b, &cases); err != nil {
		t.Fatalf("decode probe cases: %v", err)
	}
	for _, kase := range cases {
		t.Run(kase.Name, func(t *testing.T) {
			// an invalid-JSON payloadRaw is fed as the raw payload bytes verbatim
			payload := kase.Payload
			if kase.PayloadRaw != nil {
				payload = json.RawMessage(*kase.PayloadRaw)
			}
			catalog := &Catalog{Source: "probe", Format: kase.Format, Payload: payload}
			if got := probeUsable(catalog); got != kase.Usable {
				t.Fatalf("probeUsable = %v, want %v", got, kase.Usable)
			}
		})
	}
}
