# Runtime Mocking Design (Deterministic + Flexible)

## Status
- Draft for implementation.
- Source of truth for runtime mock matching, scenario state, and fallback behavior.

## Why this exists
Runtime mocking currently maps one fixture per `method + path`.
Real systems often return different responses for the same endpoint based on query, headers, auth context, body fields, and workflow state.

This document defines a protocol-agnostic model that supports those cases while preserving deterministic behavior for CI and local reproducibility.

## Scope
- Runtime mock and mock-partial behavior.
- Deterministic request matching pipeline.
- Stateful scenarios and persisted state.
- Fallback precedence and miss behavior.
- Auth context handling for RBAC/ABAC-oriented matching.

## Non-goals (v1)
- Non-deterministic response selection by default.
- Embedded scripting runtimes in matcher evaluation.
- Persisting raw bearer tokens in fixtures or runtime state.
- Protocol-specific fields in shared runtime contracts.

## Invariants (must hold)
- Shared runtime contracts remain protocol-agnostic.
- Protocol-specific extraction/validation remains inside adapters.
- Matching and scenario resolution are deterministic.
- CLI/runtime behavior is non-interactive and stable in CI.

## Terms
- Structural matchers: `method`, `path`, and required scenario state.
- Attribute matchers: query params, headers/cookies, auth context, body selectors.
- Scenario: named rule set with optional state transition and response outcome.
- Service fallback: per-service default response when no scenario matches.
- Global fallback: runtime-wide default response when no service-level result exists.

## High-level model
1. Route request to service deterministically (existing longest-prefix behavior).
2. Build canonical request context.
3. Evaluate scenario candidates in deterministic order.
4. Apply first full match.
5. Apply state transition atomically.
6. Resolve response using fallback precedence.
7. Run validation overlay (warn/strict) on selected response path.

## Canonical request context (runtime-core)
Runtime matching receives a protocol-agnostic context:

- `method`: normalized uppercase HTTP method.
- `path`: normalized path without query string.
- `query`: normalized multi-map preserving repeated keys.
- `headers`: case-insensitive map with canonical lower-case keys.
- `cookies`: parsed cookie map.
- `body`: raw bytes + optional parsed JSON value.
- `auth_context`: normalized identity/authorization fields derived per request.

`auth_context` v1 fields:
- `subject` (optional string)
- `roles` (set of strings)
- `attributes` (key/value map from JWT claims)
- `token_fingerprint` (optional, disabled by default)

## Auth handling (RBAC + ABAC)
### v1 defaults
- Decode JWT claims per request when bearer token exists.
- Do not persist raw token values in fixtures, state, logs, or reports.
- ABAC source in v1: JWT claims only.
- Token fingerprinting is optional and disabled by default.

### Redaction requirements
- Authorization header values must always be redacted in logs/errors.
- Cookie values are redacted by default.
- Query parameters commonly carrying secrets must be redacted when emitted.

## Scenario configuration model
Scenario rules are organized per service. A scenario has:

- `id`: stable identifier.
- `priority`: integer (higher first).
- `when`: matcher set.
- `state`: optional requirements and transition.
- `respond`: response descriptor.

`when` supports:
- `method`, `path`
- `query` predicates
- `headers` predicates
- `cookies` predicates
- `auth` predicates (subject/roles/attributes)
- `body_json` predicates (dot-path selectors against JSON body)
- `expr` deterministic DSL expression

`state` supports:
- `requires`: named state guard (optional)
- `set`: next state value (optional)

`respond` supports:
- Inline response (`status`, `headers`, `body`)
- Fixture reference
- Named template response (future-safe, deterministic inputs only)

## Deterministic ordering and tie-break
Scenario candidate ordering is:
1. `priority` descending.
2. Declaration order ascending.
3. `id` lexical ascending.

First full-match candidate wins.

## Matcher evaluation order
For each candidate, evaluate in fixed order:
1. `method`
2. `path`
3. `state.requires`
4. `query`
5. `headers`
6. `cookies`
7. `auth` (subject/roles/attributes)
8. `body` selectors
9. `expr` deterministic DSL

Any failure stops that candidate and proceeds to next candidate.

## Deterministic expression DSL (v1)
- Pure expression language only.
- No time/random/network/file/system calls.
- No mutation side effects.
- Stable function set and stable coercion rules.

Implemented subset in current build:
- equality clauses only (`<operand> == <literal>`)
- boolean conjunction with `&&`
- string/number/bool/null literal coercion through canonical string matching

Supported operands:
- `method`, `path`
- `query.<name>`
- `header.<name>`
- `cookie.<name>`
- `auth.subject`
- `auth.attr.<name>`
- `body.<dot.path>`

Any parse/eval error yields deterministic matcher failure for that candidate.

## Scenario state store (persisted file)
### v1 choice
- Persist scenario state in a local file store.

### Requirements
- Deterministic state key: service + scenario namespace (+ optional explicit key resolver).
- Atomic write on transition.
- Read-your-write consistency in single process.
- Defined behavior for conflicting updates (last-writer-wins with ordered request handling in runtime loop).
- Reset command/path documented for clean test runs.

### Recommended layout
- Root under runtime working directory/registry.
- Per-service JSON state files.
- Human-readable but machine-stable ordering for diffs.

## Fallback precedence
When no scenario hit produces a response, use:
1. Service-level fallback response.
2. Global fallback response.
3. Mode fallback:
   - `mock`: terminal miss (`404`, `x-specgate-mock: miss`)
   - `mock-partial`: configurable behavior

`mock-partial` default behavior:
- Upstream fallback only when no structural scenario match exists.

Optional behavior:
- Upstream fallback on any non-hit.

Outcome headers:
- `x-specgate-mock: hit | fallback | miss`
- Optional reason header for diagnostics (must not leak secrets).

## Structural match definition
Structural match in v1 = `method + path + state.requires`.

This definition controls the strict default for `mock-partial`:
- Structural match exists but candidate fails deeper matchers => no upstream by default.
- No structural match => eligible for upstream fallback.

## Validation interaction
- Validation runs after response path is resolved.
- Warn mode: add validation headers/messages, continue response.
- Strict mode: preserve existing strict semantics for request/response failures.

## Manifest-only behavior
Runtime mock behavior is scenario-manifest only.

- Required per service: `scenarios.yaml` (or `scenarios.yml` / `scenarios.json`).
- Legacy `<METHOD>__<path>.json` lookup is intentionally unsupported.
- `respond.fixture` remains supported for explicit fixture references inside scenario rules.
- DX default: when a service manifest is missing, runtime startup auto-generates a starter `scenarios.yaml` template for that service.
- Optional explicit bootstrap command: `runtime init-mocks --config <path>`.

## Security and privacy defaults
- Never store raw bearer tokens by default.
- Redact secret-like fields in logs and reports.
- Recorder (when implemented) must use explicit allowlist for sensitive headers/cookies/query fields.
- Any optional fingerprinting must be opt-in and salted.

Fingerprint control:
- `SPECGATE_TOKEN_FINGERPRINT=1` enables fingerprint calculation.
- `SPECGATE_TOKEN_FINGERPRINT_SALT=<value>` sets optional salt input.

## Implementation phases
### Phase 1 (core deterministic matching)
- Add scenario manifest loading.
- Add canonical context + matcher evaluator.
- Add fallback precedence resolver.

### Phase 2 (state + auth context)
- Add persisted state store with atomic transitions.
- Add JWT claims-based auth context extraction.
- Add redaction enforcement.

### Phase 3 (hardening)
- Expand matcher tests and edge-case diagnostics.
- Add optional fingerprinting and explicit policy flags.

## Test requirements
- Deterministic ordering and first-match semantics.
- Fallback precedence matrix coverage.
- Strict-default `mock-partial` structural fallback behavior.
- State transition correctness and persistence.
- Auth matching from JWT claims (RBAC + ABAC claims).
- Redaction invariants in logs/errors.

## Open extension points (future)
- Additional ABAC sources (headers/query/body/external provider) behind explicit config.
- Optional deterministic traffic recording into scenario manifests.
- Adapter-provided protocol hints without changing runtime-core matcher contracts.

## Suggested implementation touchpoints
- `src/runtime_server.rs` (orchestration + response resolution)
- `src/runtime_matching.rs` (deterministic ordering helpers)
- `src/spec_adapters/mod.rs` (protocol adapter extension hooks)
- `src/cli.rs` (flags/config for fallback mode and auth handling)
- `tests/runtime_server.rs` and `tests/adapter_contract.rs`
