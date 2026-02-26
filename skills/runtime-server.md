# Skill: Runtime Server (Proxy/Mock)

## Scope
- Proxy mode forwards to upstream and records traffic.
- Mock mode (full) serves recorded fixtures or generated examples and returns 404 when route/fixture is missing.
- Mock partial mode serves fixture-first and falls back to upstream when fixture is missing.
- Inject local-dev CORS headers.
- Keep runtime core protocol-neutral; protocol-specific request/response semantics must be supplied by adapters.
- Resolve requests to service context using configurable URL-prefix rules, then fetch that service's active spec for validation.

## Acceptance focus
- Mode-specific behavior is explicit and testable.
- Recorder output is reusable by mock mode.
- Full mock miss semantics are deterministic (404 with observable mock outcome header).
- Partial mock fallback behavior is deterministic (fixture hit vs upstream fallback is observable).
- CORS behavior is configurable and predictable.
- Request-to-service matching is deterministic (longest prefix wins).
- Supporting additional spec protocols does not require redesign of proxy/mock core flow.

## Test focus
- Integration tests for proxy forwarding and fixture capture.
- Integration tests for full mock serving from fixtures and 404 behavior on misses.
- Integration tests for partial mock fixture miss fallback to upstream.
- Header behavior tests for CORS.
