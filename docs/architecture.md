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

## Specification architecture invariants
- OpenAPI is an initial adapter, not a global system assumption.
- Shared layers (CLI, spec registry core, runtime core, reporting core) must remain protocol-agnostic.
- Protocol-specific concerns (detection, parsing, semantic validation, mapping to internal contracts) must be isolated behind adapter modules.
- Internal contracts used across boundaries must not include protocol-specific field names (for example, do not hardcode OpenAPI-only semantics in shared structs).
- Adding support for a new protocol/version should require a new adapter implementation, not redesign of shared modules.

## Compatibility direction
- Initial adapter: OpenAPI
- Required direction: OpenAPI (relevant versions), gRPC, GraphQL
- Planned codegen target expansion beyond Java/TypeScript
