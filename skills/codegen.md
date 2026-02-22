# Skill: Code Generation

## Scope
- Generate typed client/server models/code.
- Prioritize Java target first.
- Keep TypeScript as reference target.
- Define plugin-like target interface for extensibility.
- Consume protocol-neutral intermediate contracts so generators are not coupled to OpenAPI-specific source structures.

## Acceptance focus
- Deterministic output for same input.
- Stable target contract for future generators.
- CI-friendly non-interactive commands.
- New protocol adapters (OpenAPI/GraphQL/gRPC) plug in without redesigning generator interfaces.

## Test focus
- Golden-file tests for generated outputs.
- Integration tests for CLI invocation and exit codes.
