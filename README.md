# specgate

`specgate` is a Rust CLI + runtime toolkit for multi-spec development workflows (OpenAPI, GraphQL, gRPC) with adapter-isolated protocol logic.

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
5. Keep shared modules protocol-agnostic; place protocol/version-specific logic only in adapters.

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

## Spec management (first feature)

### Add a spec from local file

```bash
cargo run -- spec add --service petstore --file ./openapi.json
```

### Add a spec from URL

```bash
cargo run -- spec add --service petstore --url https://petstore3.swagger.io/api/v3/openapi.json
```

### Add a spec from URL with auth

```bash
cargo run -- spec add --service internal --url https://example.com/v3/api-docs --auth-bearer <token>
```

```bash
cargo run -- spec add --service internal --url https://example.com/v3/api-docs --auth-basic <user:pass>
```

```bash
cargo run -- spec add --service internal --url https://example.com/v3/api-docs --auth-apikey-header <header:value>
```

```bash
cargo run -- spec add --service internal --url https://example.com/v3/api-docs --auth-apikey-query <key:value>
```

### List stored specs

```bash
cargo run -- spec list
```

### Public endpoints for manual testing

- OpenAPI 3 JSON: https://petstore3.swagger.io/api/v3/openapi.json
- OpenAPI 3 YAML: https://raw.githubusercontent.com/swagger-api/swagger-petstore/master/src/main/resources/openapi.yaml
- Swagger 2 JSON (expected reject in current version): https://petstore.swagger.io/v2/swagger.json

## Versioning approach

- Current approach: Semantic Versioning (SemVer).
- Initial development: `0.y.z` where minor releases may include breaking changes.
- Stable contract phase: move to `1.0.0` and keep breaking changes for major bumps.

## License

This project is licensed under AGPL-3.0:

- See [LICENSE](LICENSE).
- Optional service-provider commercial terms are described in [LICENSE-ADDENDUM.md](LICENSE-ADDENDUM.md).
- If you run a managed hosted service based on this project, maintainers welcome a direct commercial conversation.
