# Probe-contract conformance fixtures

Shared gold for the unusable-payload rule: a raw source payload is usable when
parsing it in the named format produces at least one model. A payload that is
malformed, or that parses to zero models, is unusable. This fixture pins that
rule so any consumer that drifts fails against shared gold.

## `cases.json`

An array of cases, each:

- `name`: a stable case identifier.
- `format`: the payload format, one of `openrouter`, `litellm`, `modelsdev`.
- `payload`: the raw source JSON value, inline.
- `usable`: the expected result. `true` when the payload parses to at least one
  model, `false` when it parses to zero models or fails to parse at all.

### Invalid-JSON case

An invalid-JSON payload cannot be expressed as a JSON value, so its case carries
`payloadRaw` (a string) instead of `payload`. A consumer passes `payloadRaw` as
the raw payload bytes verbatim. The parse fails, so the payload is unusable. A
case has exactly one of `payload` or `payloadRaw`.

## The cases

- `openrouter-valid`, `litellm-valid`, `modelsdev-valid`: trimmed valid
  payloads in each format's on-the-wire shape. Each parses to at least one
  model (usable).
- `openrouter-zero-models`: a syntactically valid OpenRouter payload with an
  empty `data` array (zero models, unusable).
- `litellm-zero-models`: a LiteLLM payload carrying only `sample_spec`, which
  is skipped (zero models, unusable).
- `modelsdev-zero-models`: an empty models.dev object with no providers (zero
  models, unusable).
- `openrouter-empty-object`: an empty object under the OpenRouter format,
  distinct from the empty-`data` case above (no `data` array, zero models,
  unusable).
- `invalid-json`: a `payloadRaw` string that is not valid JSON (the parse
  fails, unusable).

For LiteLLM and models.dev the empty object is itself the zero-model case, so no
separate empty-object case is carried for those formats.
