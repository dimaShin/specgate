# Skill: Validation & Reporting

## Scope
- Validate inbound/outbound payloads against the active spec adapter at runtime.
- Emit warnings by default without crashing proxy.
- Support strict mode to fail on validation errors.
- Write summary report during server run and expose later via CLI.

## Acceptance focus
- Validation pipeline supports request + response checks.
- Report file updates incrementally while server runs.
- CLI can inspect/report historical run data.
- Shared validation/reporting contracts remain protocol-agnostic while protocol-specific rules live in adapters.

## Test focus
- Runtime integration tests with valid/invalid payloads.
- Strict vs warning-only behavior tests.
- Report persistence and readback tests.
