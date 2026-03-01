# Skill: Runtime Server (Proxy/Mock)

Primary design reference for flexible mocking behavior:
- `docs/runtime-mocking-design.md`

## Scope
- Proxy mode forwards to upstream and records traffic.
- Mock mode (full) serves deterministic scenario/fixture responses and returns 404 when no response path is resolved.
- Mock partial mode serves deterministic scenario/fixture responses and falls back according to configured precedence.
- Inject local-dev CORS headers.
- Keep runtime core protocol-neutral; protocol-specific request/response semantics must be supplied by adapters.
- Resolve requests to service context using configurable URL-prefix rules, then fetch that service's active spec for validation.
- Support stateful scenarios with persisted state transitions.
- Support auth-aware matching (RBAC/ABAC), with security-first token handling defaults.

## Acceptance focus
- Mode-specific behavior is explicit and testable.
- Recorder output is reusable by mock mode.
- Deterministic candidate ordering and first-match semantics are explicit and testable.
- Full mock miss semantics are deterministic (404 with observable mock outcome header).
- Partial mock fallback behavior is deterministic and configurable (strict structural fallback default).
- CORS behavior is configurable and predictable.
- Request-to-service matching is deterministic (longest prefix wins).
- Supporting additional spec protocols does not require redesign of proxy/mock core flow.

## Test focus
- Integration tests for proxy forwarding and fixture capture.
- Integration tests for deterministic scenario matching and tie-break order.
- Integration tests for full mock serving and 404 behavior on misses.
- Integration tests for partial mock fallback precedence (strict default + optional permissive mode).
- Integration tests for state transitions and persisted state behavior.
- Integration tests for auth-context matching and secret redaction invariants.
- Header behavior tests for CORS.
