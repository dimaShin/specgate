# Context Map (load on demand)

Use this to keep agent context minimal while still discoverable.

## Default
- Always load: `AGENTS.md` + `docs/project-brief.md` + `docs/architecture.md`
- Do not load additional files unless required by task scope.
- Keep spec-related decisions protocol-agnostic unless a task is explicitly adapter-specific.

## Routing by task

### Spec registry / fetch / local storage
- `skills/spec-management.md`
- For any spec-facing refactor, keep `docs/architecture.md` in active context to enforce adapter isolation invariants.

### Code generation / targets / determinism
- `skills/codegen.md`

### Proxy or mock runtime / traffic recording / CORS
- `skills/runtime-server.md`

### Runtime schema validation / warning-strict behavior / report lifecycle
- `skills/validation-reporting.md`

### Refactor affecting boundaries or extensibility model
- Then only the impacted skill docs
- Always verify shared modules remain protocol-neutral and protocol logic remains in adapters.

### Product-level direction questions
- `docs/project-brief.md`

## Manual mode (strictest)
If you prefer full manual control, load only:
1. `AGENTS.md`
2. `docs/project-brief.md`
3. `docs/architecture.md`
4. Exactly one targeted skill file
