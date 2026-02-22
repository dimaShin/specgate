# Skill: Spec Management

## Scope
- Own only spec ingestion and local registry behavior.
- Include URL fetch from day 0 (not optional).
- Keep boundaries strict: CLI + spec registry + fetch adapter only.
- Keep shared ingestion/registry contracts protocol-agnostic; route protocol/version detection and normalization through isolated adapters.

## Layer boundaries
- CLI layer: parse `spec` commands, validate flags, map user errors to stable messages.
- Spec registry: persist and list spec entries deterministically from local storage using protocol-neutral metadata.
- Fetch adapter: download remote spec bytes with explicit auth options and timeout behavior.
- Spec adapters: detect protocol/version and provide normalized identity metadata (OpenAPI/GraphQL/gRPC and future variants).
- Out of scope for this skill: codegen, runtime server, validation, recorder, reporting.

## POC (first shipped slice)
- Commands: `spec add --service <id> --file <path>`, `spec add --service <id> --url <url>`, `spec list`.
- Registry default: project-local directory.
- Service identity: required user-provided service id.
- Version model: every ingest creates a version entry (no in-place overwrite).
- Initial adapter support: OpenAPI 3.0/3.1.
- OpenAPI 2.0, GraphQL, and gRPC behavior (until adapters ship): reject with actionable error that names unsupported protocol/version.
- Fetch auth support from day 0: bearer, basic, API key (header/query).

## Acceptance focus
- Deterministic add/list behavior with stable output ordering.
- Deterministic local filesystem layout for service + version entries.
- Clear failure messages for file IO, URL fetch, auth, timeout, and unsupported protocol/version.
- Output model includes protocol-neutral identity fields (for example: spec kind + declared version) so later adapters do not require schema redesign.

## MVP (next)
- Add `spec show` and `spec remove` commands.
- Add duplicate-content detection and optional dedupe mode.
- Add explicit source/provenance output (file/url, fetched time, digest) in CLI views.
- Add protocol/version listing fields in CLI output and metadata API.

## Product roadmap
- Add discovery-oriented ingestion workflows for Java microservices (e.g., conventional docs endpoints and source lists).
- Add policy controls for refresh/sync across environments.
- Add migration path for OpenAPI 2.0 import (store or convert strategy).
- Implement GraphQL and gRPC adapters without changing shared registry core contracts.

## Test focus
- Integration tests for `spec` CLI flows and stable exit codes.
- Fixture-based URL fetch tests (including auth and failure paths).
- Registry determinism tests (ordering, version append behavior, reproducible layout).
- Adapter contract tests to guarantee new protocol support can be added without changing shared module interfaces.
