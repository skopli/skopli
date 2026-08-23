// Package skopli binds the skopli C ABI (the `ag_*` surface generated
// from crates/skopli-capi) via cgo and presents an idiomatic Go facade.
//
// This file is the thin cgo layer: it owns every unsafe FFI call, the AgBuf /
// C-string ownership discipline (library-owned outputs are copied into Go
// []byte/string and freed via ag_buf_free / ag_string_free), and the AgStatus ->
// Go error mapping. Everything above it (typed structs, JSON marshalling, the
// Pricing handle, the pricing-source seam) is pure Go in the sibling files.
//
// Link mechanics (documented in the SDK README): the capi
// cdylib must be built for the MinGW/GNU target so cgo's gcc/ld can link it:
//
//	cargo build -p skopli-capi --release --target x86_64-pc-windows-gnu
//
// then the DLL directory must be on PATH (Windows) / LD_LIBRARY_PATH (Linux) at
// run time. The #cgo directives below point the compiler at the header and the
// GNU import lib in the workspace target dir; override with CGO_CFLAGS /
// CGO_LDFLAGS for out-of-tree layouts.
package skopli

/*
#cgo CFLAGS: -I${SRCDIR}/../../../crates/skopli-capi/include
#cgo windows LDFLAGS: -L${SRCDIR}/../../../target/x86_64-pc-windows-gnu/release -lskopli
#cgo !windows LDFLAGS: -L${SRCDIR}/../../../target/release -lskopli
#include <stdlib.h>
#include <string.h>
#include "skopli.h"
*/
import "C"

import (
	"errors"
	"fmt"
	"unsafe"
)

// Sentinel errors mapped from the C ABI AgStatus codes. Facade calls wrap these
// with the thread-local ag_last_error_message detail, so callers use errors.Is
// against these values.
var (
	// ErrInvalidArgument corresponds to AG_STATUS_INVALID_ARGUMENT (1): a bad
	// argument - null where required, malformed JSON, an out-of-range date/tz,
	// or a bad enum value.
	ErrInvalidArgument = errors.New("skopli: invalid argument")
	// ErrCatalog corresponds to AG_STATUS_CATALOG (2): a pricing/catalog
	// operation failed (e.g. unparseable catalog JSON).
	ErrCatalog = errors.New("skopli: catalog error")
	// ErrInternal corresponds to AG_STATUS_INTERNAL (3): an unexpected internal
	// error, including a caught panic in the native library.
	ErrInternal = errors.New("skopli: internal error")
	// ErrClosed is returned by any query on a Pricing handle that has been
	// closed. Close is terminal: a closed handle never rebuilds a native handle.
	ErrClosed = errors.New("skopli: pricing handle closed")
)

// sentinelFor maps a non-OK AgStatus to its Go sentinel.
func sentinelFor(status C.AgStatus) error {
	switch status {
	case C.AG_STATUS_INVALID_ARGUMENT:
		return ErrInvalidArgument
	case C.AG_STATUS_CATALOG:
		return ErrCatalog
	default:
		return ErrInternal
	}
}

// statusError turns a non-OK status into a wrapped sentinel carrying the
// thread-local detail message, so both errors.Is(err, ErrX) and a readable
// message work.
func statusError(status C.AgStatus) error {
	sentinel := sentinelFor(status)
	if detail := lastErrorMessage(); detail != "" {
		return fmt.Errorf("%w: %s", sentinel, detail)
	}
	return sentinel
}

// lastErrorMessage reads and frees this thread's last native error detail, or
// returns "" when none was recorded.
func lastErrorMessage() string {
	var out *C.char
	if C.ag_last_error_message(&out) != C.AG_STATUS_OK || out == nil {
		return ""
	}
	msg := C.GoString(out)
	C.ag_string_free(out)
	return msg
}

// bufToBytes copies a library-owned AgBuf into a freshly allocated Go []byte and
// frees the native buffer. A zero buffer yields an empty (non-nil) slice.
func bufToBytes(buf C.AgBuf) []byte {
	defer C.ag_buf_free(buf)
	if buf.ptr == nil || buf.len == 0 {
		return []byte{}
	}
	return C.GoBytes(unsafe.Pointer(buf.ptr), C.int(buf.len))
}

// abiVersion returns the native ABI version integer.
func abiVersion() uint32 { return uint32(C.ag_abi_version()) }

// schemaVersion returns the conformance schema_version the native build emits.
func schemaVersion() uint32 { return uint32(C.ag_schema_version()) }

// version returns the native library semver string (a 'static C string, never
// freed).
func version() string { return C.GoString(C.ag_version()) }

// callJSONOpts invokes one of the single-options-in / JSON-out ABI functions
// (detect, read_usage). optsJSON may be nil (null options = all defaults).
func callJSONOpts(
	fn func(opts *C.char, optsLen C.uintptr_t, out *C.AgBuf) C.AgStatus,
	optsJSON []byte,
) ([]byte, error) {
	optsPtr, optsLen, free := borrowBytes(optsJSON)
	defer free()
	var out C.AgBuf
	if status := fn(optsPtr, optsLen, &out); status != C.AG_STATUS_OK {
		return nil, statusError(status)
	}
	return bufToBytes(out), nil
}

// detectHarnessesRaw calls ag_detect_harnesses.
func detectHarnessesRaw(optsJSON []byte) ([]byte, error) {
	return callJSONOpts(func(o *C.char, l C.uintptr_t, out *C.AgBuf) C.AgStatus {
		return C.ag_detect_harnesses(o, l, out)
	}, optsJSON)
}

// readUsageRaw calls ag_read_usage.
func readUsageRaw(optsJSON []byte) ([]byte, error) {
	return callJSONOpts(func(o *C.char, l C.uintptr_t, out *C.AgBuf) C.AgStatus {
		return C.ag_read_usage(o, l, out)
	}, optsJSON)
}

// rollupRaw calls ag_rollup with an events array and rollup options.
func rollupRaw(eventsJSON, optsJSON []byte) ([]byte, error) {
	ePtr, eLen, freeE := borrowBytes(eventsJSON)
	defer freeE()
	oPtr, oLen, freeO := borrowBytes(optsJSON)
	defer freeO()
	var out C.AgBuf
	if status := C.ag_rollup(ePtr, eLen, oPtr, oLen, &out); status != C.AG_STATUS_OK {
		return nil, statusError(status)
	}
	return bufToBytes(out), nil
}

// costUSDRaw calls ag_cost_usd, returning the scalar cost.
func costUSDRaw(tokensJSON, priceJSON []byte) (float64, error) {
	tPtr, tLen, freeT := borrowBytes(tokensJSON)
	defer freeT()
	pPtr, pLen, freeP := borrowBytes(priceJSON)
	defer freeP()
	var out C.double
	if status := C.ag_cost_usd(tPtr, tLen, pPtr, pLen, &out); status != C.AG_STATUS_OK {
		return 0, statusError(status)
	}
	return float64(out), nil
}

// defaultCacheDirRaw calls ag_default_cache_dir.
func defaultCacheDirRaw() (string, error) {
	var out *C.char
	if status := C.ag_default_cache_dir(&out); status != C.AG_STATUS_OK {
		return "", statusError(status)
	}
	defer C.ag_string_free(out)
	return C.GoString(out), nil
}

// pricingHandle is the opaque native AgPricing pointer wrapper.
type pricingHandle struct {
	ptr *C.AgPricing
}

// pricingNewRaw calls ag_pricing_new, returning an owned native handle.
func pricingNewRaw(optsJSON []byte) (*pricingHandle, error) {
	optsPtr, optsLen, free := borrowBytes(optsJSON)
	defer free()
	var handle *C.AgPricing
	if status := C.ag_pricing_new(optsPtr, optsLen, &handle); status != C.AG_STATUS_OK {
		return nil, statusError(status)
	}
	return &pricingHandle{ptr: handle}, nil
}

// free releases the native handle. Idempotent: a nil handle is a no-op.
func (h *pricingHandle) free() {
	if h != nil && h.ptr != nil {
		C.ag_pricing_free(h.ptr)
		h.ptr = nil
	}
}

// priceEventsRaw calls ag_pricing_price_events on a live handle.
func (h *pricingHandle) priceEventsRaw(eventsJSON, optsJSON []byte) ([]byte, error) {
	ePtr, eLen, freeE := borrowBytes(eventsJSON)
	defer freeE()
	oPtr, oLen, freeO := borrowBytes(optsJSON)
	defer freeO()
	var out C.AgBuf
	status := C.ag_pricing_price_events(h.ptr, ePtr, eLen, oPtr, oLen, &out)
	if status != C.AG_STATUS_OK {
		return nil, statusError(status)
	}
	return bufToBytes(out), nil
}

// priceRollupsRaw calls ag_pricing_price_rollups on a live handle.
func (h *pricingHandle) priceRollupsRaw(rollupsJSON []byte) ([]byte, error) {
	rPtr, rLen, freeR := borrowBytes(rollupsJSON)
	defer freeR()
	var out C.AgBuf
	status := C.ag_pricing_price_rollups(h.ptr, rPtr, rLen, &out)
	if status != C.AG_STATUS_OK {
		return nil, statusError(status)
	}
	return bufToBytes(out), nil
}

// lookupModelRaw calls ag_pricing_lookup_model on a live handle with a bare
// model-name buffer.
func (h *pricingHandle) lookupModelRaw(model []byte) ([]byte, error) {
	mPtr, mLen, freeM := borrowBytes(model)
	defer freeM()
	var out C.AgBuf
	status := C.ag_pricing_lookup_model(h.ptr, mPtr, mLen, &out)
	if status != C.AG_STATUS_OK {
		return nil, statusError(status)
	}
	return bufToBytes(out), nil
}

// catalogInfoRaw calls ag_pricing_catalog_info on a live handle.
func (h *pricingHandle) catalogInfoRaw() ([]byte, error) {
	var out C.AgBuf
	if status := C.ag_pricing_catalog_info(h.ptr, &out); status != C.AG_STATUS_OK {
		return nil, statusError(status)
	}
	return bufToBytes(out), nil
}

// borrowBytes hands the C ABI a (ptr, len) view of a Go []byte for the duration
// of a call. The ABI treats a null/zero pair as "null options = defaults", so a
// nil/empty slice becomes (nil, 0). A non-empty slice is copied into C memory
// (cgo forbids passing a Go pointer that itself contains a Go pointer, and a
// stable non-moving pointer is required across the call); the returned free
// must be deferred.
func borrowBytes(b []byte) (*C.char, C.uintptr_t, func()) {
	if len(b) == 0 {
		return nil, 0, func() {}
	}
	ptr := C.CBytes(b)
	return (*C.char)(ptr), C.uintptr_t(len(b)), func() { C.free(ptr) }
}
