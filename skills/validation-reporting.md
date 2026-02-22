# Skill: Validation & Reporting

## Scope
- Validate inbound/outbound payloads against OpenAPI at runtime.
- Emit warnings by default without crashing proxy.
- Support strict mode to fail on validation errors.
- Write summary report during server run and expose later via CLI.

## Acceptance focus
- Validation pipeline supports request + response checks.
- Report file updates incrementally while server runs.
- CLI can inspect/report historical run data.

## Test focus
- Runtime integration tests with valid/invalid payloads.
- Strict vs warning-only behavior tests.
- Report persistence and readback tests.
