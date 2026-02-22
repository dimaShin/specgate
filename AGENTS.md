# specgate agent bootstrap (minimal default context)

Load this file, `docs/project-brief.md`, and `docs/architecture.md` by default for every agent.
Do not auto-load the full docs set beyond those three files.

## Project one-liner
`specgate` is a Rust CLI + runtime for OpenAPI-driven spec management, code generation, proxy/mock serving, runtime validation, and reporting.

## Non-negotiables
- Rust, no `unsafe`.
- Must run on macOS dev and common Linux CI runners.
- Deterministic, non-interactive CI behavior with stable exit codes.
- Keep changes small and compiling.
- Add tests per feature (integration tests for CLI/server behavior).

## Architecture boundaries (must stay separate)
- CLI layer
- Spec registry
- Codegen
- Server runtime
- Validation
- Recorder
- Reporting

## Context loading policy
1. Start with `AGENTS.md`, `docs/project-brief.md`, and `docs/architecture.md`.
2. Load additional docs only for the active task.
3. Prefer one skill doc at a time.
4. If scope spans multiple boundaries, load only the required skill docs.

## Where to look next (on demand)
- Architecture guardrails: `docs/architecture.md`
- Task-to-doc routing: `docs/context-map.md`
- Skill docs index: `skills/README.md`
