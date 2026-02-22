# Architecture Guardrails

## Required boundaries
- CLI layer
- Spec registry
- Code generation
- Server runtime
- Validation
- Recorder
- Reporting

## Cross-cutting requirements
- Extensibility from day 0
- Deterministic CI behavior
- Stable exit codes
- Non-interactive operation by default in CI contexts
- Warnings-first validation behavior (strict mode opt-in)

## Compatibility direction
- Initial contract source: OpenAPI
- Planned future protocols: OpenAPI 2.0, gRPC, GraphQL
- Planned codegen target expansion beyond Java/TypeScript
