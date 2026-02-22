# Project Brief: specgate

## Platform
- Development: macOS
- Runtime/CI: common Linux distros (CI runners)
- Release: single static-ish binary

## Goal
Build a Rust CLI tool that:

1. Manages API specs (store locally, optionally fetch from URL), starting with OpenAPI and extending to GraphQL and gRPC without redesign.
2. Generates typed client/server models/code for multiple languages (initial focus: Java; TS as reference; extensible plugin-like architecture).
3. Runs an HTTP server in two modes:
   - proxy mode: forwards traffic to upstream, records requests/responses to filesystem for later use as mocks
   - mock mode: serves recorded fixtures (or generated examples) without upstream
4. Validates inbound/outbound payloads against the active spec adapter at runtime, emits warnings, and writes a summary report.
5. Solves CORS for local dev by injecting appropriate CORS headers.
6. Works in CI: deterministic output, exit codes, non-interactive operation.

## Development workflow
- Keep changes small and compiling.
- Add tests per feature, especially integration tests for CLI and server behavior.

## Architecture requirements
- Clear separation: CLI layer / spec registry / codegen / server runtime / validation / recorder / reporting.
- Extensibility from day 0: support multiple codegen targets and protocol sources (OpenAPI including 2.0/3.x, GraphQL, gRPC) without redesign.
- Shared modules (CLI, registry core, runtime core, reporting core) must remain protocol-agnostic; protocol-specific logic belongs only in isolated adapters.

## Quality constraints
- No unsafe.
- Minimal dependencies, well-justified.
- Helpful error messages and stable CLI UX.
- Runtime validation produces warnings without crashing the proxy, unless configured.
- Report must be writable during server run and viewable later via CLI.
