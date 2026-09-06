package skopli

import (
	"encoding/json"
	"fmt"
)

// ABIVersion returns the native ABI version integer (bumped on a breaking ABI
// change).
func ABIVersion() uint32 { return abiVersion() }

// SchemaVersion returns the conformance schema_version the native build emits.
func SchemaVersion() uint32 { return schemaVersion() }

// Version returns the native library semver string.
func Version() string { return version() }

// DefaultCacheDir returns the default on-disk pricing cache directory
// (platform-specific).
func DefaultCacheDir() (string, error) { return defaultCacheDirRaw() }

// Options carries the {home, env} path-resolution seam plus the read filters
// shared by DetectHarnesses and ReadUsage. All fields are optional; the zero
// Options means "process defaults". Home and Env travel as plain data in the
// options JSON (the path-resolver seam lives facade-side).
type Options struct {
	// Home overrides the resolved home directory.
	Home string `json:"home,omitempty"`
	// Env is the environment map the readers resolve their data dirs from. The
	// native side cannot read the parent process env, so this must be the full
	// environment when overriding.
	Env map[string]string `json:"env,omitempty"`
}

// ReadUsageOptions drives ReadUsage: which harnesses, the since/until window,
// the timezone for date bounds, and subagent handling.
type ReadUsageOptions struct {
	Options
	// Harnesses restricts the read to these harness ids; empty means detect.
	Harnesses []Harness `json:"harnesses,omitempty"`
	// Since is an inclusive lower bound (a YYYY-MM-DD date or an ISO timestamp).
	Since string `json:"since,omitempty"`
	// Until is an exclusive upper bound (a YYYY-MM-DD date or an ISO timestamp).
	Until string `json:"until,omitempty"`
	// TZ is the IANA timezone for date-only bound resolution and day bucketing
	// (empty defaults to UTC, the hermetic default).
	TZ string `json:"tz,omitempty"`
	// Subagents set to "exclude" drops subagent events.
	Subagents string `json:"subagents,omitempty"`
}

// RollupOptions selects the rollup dimension and (for day/block) the timezone.
// BlockMs sets the billing-block width for ByBlock (0 uses DefaultBlockMs).
type RollupOptions struct {
	By      RollupBy `json:"by"`
	TZ      string   `json:"tz,omitempty"`
	BlockMs int64    `json:"blockMs,omitempty"`
}

// DetectHarnesses reports which registered harnesses have readable data
// reachable from opts. A nil opts uses process defaults.
func DetectHarnesses(opts *Options) (Detection, error) {
	var raw []byte
	if opts != nil {
		var err error
		if raw, err = json.Marshal(opts); err != nil {
			return Detection{}, fmt.Errorf("%w: marshalling options: %v", ErrInvalidArgument, err)
		}
	}
	out, err := detectHarnessesRaw(raw)
	if err != nil {
		return Detection{}, err
	}
	var detection Detection
	if err := json.Unmarshal(out, &detection); err != nil {
		return Detection{}, fmt.Errorf("%w: decoding detection: %v", ErrInternal, err)
	}
	return detection, nil
}

// ReadUsage reads usage across the selected harnesses, returning the
// {events, diagnostics, skipped} envelope. Malformed input never errors; it is
// reported in Diagnostics/Skipped. Errors surface only for programmer mistakes
// (bad date/tz) and I/O-fatal conditions.
func ReadUsage(opts ReadUsageOptions) (ReadUsageResult, error) {
	raw, err := json.Marshal(opts)
	if err != nil {
		return ReadUsageResult{}, fmt.Errorf("%w: marshalling options: %v", ErrInvalidArgument, err)
	}
	out, err := readUsageRaw(raw)
	if err != nil {
		return ReadUsageResult{}, err
	}
	var result ReadUsageResult
	if err := json.Unmarshal(out, &result); err != nil {
		return ReadUsageResult{}, fmt.Errorf("%w: decoding read result: %v", ErrInternal, err)
	}
	return result, nil
}

// Rollup aggregates events by the option dimension. It cannot fail for
// well-formed events but returns an error for a bad dimension.
func Rollup(events []UsageEvent, opts RollupOptions) ([]RollupBucket, error) {
	eventsJSON, err := json.Marshal(events)
	if err != nil {
		return nil, fmt.Errorf("%w: marshalling events: %v", ErrInvalidArgument, err)
	}
	optsJSON, err := json.Marshal(opts)
	if err != nil {
		return nil, fmt.Errorf("%w: marshalling options: %v", ErrInvalidArgument, err)
	}
	out, err := rollupRaw(eventsJSON, optsJSON)
	if err != nil {
		return nil, err
	}
	var rollups []RollupBucket
	if err := json.Unmarshal(out, &rollups); err != nil {
		return nil, fmt.Errorf("%w: decoding rollups: %v", ErrInternal, err)
	}
	return rollups, nil
}

// CostUSD computes the USD cost of a token bundle under a flat or tiered price.
func CostUSD(tokens TokenCounts, price ModelPrice) (float64, error) {
	tokensJSON, err := json.Marshal(tokens)
	if err != nil {
		return 0, fmt.Errorf("%w: marshalling tokens: %v", ErrInvalidArgument, err)
	}
	priceJSON, err := json.Marshal(price)
	if err != nil {
		return 0, fmt.Errorf("%w: marshalling price: %v", ErrInvalidArgument, err)
	}
	return costUSDRaw(tokensJSON, priceJSON)
}
