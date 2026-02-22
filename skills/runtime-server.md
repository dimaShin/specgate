# Skill: Runtime Server (Proxy/Mock)

## Scope
- Proxy mode forwards to upstream and records traffic.
- Mock mode serves recorded fixtures or generated examples.
- Inject local-dev CORS headers.
- Keep runtime core protocol-neutral; protocol-specific request/response semantics must be supplied by adapters.
- Resolve requests to service context using configurable URL-prefix rules, then fetch that service's active spec for validation.

## Acceptance focus
- Mode-specific behavior is explicit and testable.
- Recorder output is reusable by mock mode.
- CORS behavior is configurable and predictable.
- Request-to-service matching is deterministic (longest prefix wins).
- Supporting additional spec protocols does not require redesign of proxy/mock core flow.

## Test focus
- Integration tests for proxy forwarding and fixture capture.
- Integration tests for mock serving from fixtures.
- Header behavior tests for CORS.
