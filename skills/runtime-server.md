# Skill: Runtime Server (Proxy/Mock)

## Scope
- Proxy mode forwards to upstream and records traffic.
- Mock mode serves recorded fixtures or generated examples.
- Inject local-dev CORS headers.

## Acceptance focus
- Mode-specific behavior is explicit and testable.
- Recorder output is reusable by mock mode.
- CORS behavior is configurable and predictable.

## Test focus
- Integration tests for proxy forwarding and fixture capture.
- Integration tests for mock serving from fixtures.
- Header behavior tests for CORS.
