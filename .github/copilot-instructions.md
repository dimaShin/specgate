# specgate: minimal default agent context

Default behavior for all agents in this repository:

1. Keep default context minimal.
2. Always load `AGENTS.md`, `docs/project-brief.md`, and `docs/architecture.md`.
3. Do not automatically load full project docs beyond those three files.
4. Pull additional context only via `docs/context-map.md`.
5. Treat specification handling as protocol-agnostic by default: OpenAPI, GraphQL, and gRPC must be supported through isolated adapters, while shared modules stay generic.

Task routing:
- Product direction: `docs/project-brief.md`
- Architecture boundaries/cross-cutting concerns: `docs/architecture.md`
- Feature-specific work: one file from `skills/`

Execution style:
- Keep changes small and compiling.
- Prefer deterministic, non-interactive behavior.
- Add tests close to changed behavior, especially CLI/server integration tests.
- Never add protocol-specific parsing/validation/generation logic directly into generic modules (CLI, registry core, runtime core, reporting core).
