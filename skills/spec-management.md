# Skill: Spec Management

## Scope
- Store OpenAPI specs locally.
- Fetch specs from URL optionally.
- Support deterministic local registry behavior.

## Acceptance focus
- CLI commands for add/list/show/remove.
- Stable filesystem layout.
- Network fetch with clear failure messages.

## Test focus
- Integration tests for CLI commands.
- Fixture-based tests for URL fetch and local cache behavior.
