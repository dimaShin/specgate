# specgate

`specgate` is a Rust CLI + runtime toolkit for OpenAPI-driven development workflows.

## Agent context strategy

- Default minimal context for every agent: `AGENTS.md`
- Load additional docs only when task scope requires them
- Use `docs/context-map.md` to select exactly which file(s) to add

## Docs map

- Minimal bootstrap for all agents: `AGENTS.md`
- Task-to-doc routing index: `docs/context-map.md`
- Canonical high-level brief: `docs/project-brief.md`
- Architecture constraints: `docs/architecture.md`
- Skill-oriented feature context: `skills/`
- Reusable skill template: `templates/skill-template.md`

## How to use this repo context

1. Start every agent with `AGENTS.md` only.
2. Add just one targeted skill doc (and `docs/architecture.md` only if needed).
3. Keep product-level direction in `docs/project-brief.md`.
4. Keep implementation details small and test-driven.

## Initial CLI scaffold

This repository now includes a minimal Rust CLI baseline for initial development.

### Run the version command

```bash
cargo run -- version
```

### Run tests

```bash
cargo test
```

## Versioning approach

- Current approach: Semantic Versioning (SemVer).
- Initial development: `0.y.z` where minor releases may include breaking changes.
- Stable contract phase: move to `1.0.0` and keep breaking changes for major bumps.

## License

This project is licensed under AGPL-3.0:

- See [LICENSE](LICENSE).
- Optional service-provider commercial terms are described in [LICENSE-ADDENDUM.md](LICENSE-ADDENDUM.md).
- If you run a managed hosted service based on this project, maintainers welcome a direct commercial conversation.
