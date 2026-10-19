<!-- wairon-version: 6.0.0 -->
<!-- wairon-generated — do not edit directly; the human developer rebuilds this with `wairon generate` -->

# Domain Map (4 domains)

| ID | Source | Name |
|----|--------|------|
| `distribution` | subsystem `distribution` | Distribution |
| `project` | subsystem `project` | Project |
| `publishing` | subsystem `publishing` | Publishing |
| `session` | subsystem `session` | Session |

---

## wairon — Spec-Driven Development (optional)

If `.wai/specs/` exists, the wairon SDD workflow is active; otherwise ignore it. wairon does not orchestrate sessions — it equips yours.

### In SDD Projects:
- **Source of Truth**: All architecture lives in the spec tree under `.wai/specs/` (L0 System → L1 Subsystem → L2 Component → L3 Interface → L4 Implementation → L5 Narrative). Agent files under `.claude/agents/` are an opt-in generated view (`rules.materializeAgentFiles`) — never edit them; agents are served as live briefs.
- **Validation**: Conformance checks (stereotype rules, cycle checks, reference integrity) are run via the `sdd_validate_tree` MCP tool.
- **Operating Rules**:
  1. **Skills**: Use `sdd-architect` to design (and `sdd-implement`, `sdd-narrative`, `sdd-auditor`). Refer to project's local guide file for detailed constraints.
  2. **MCP Tools Only**: Author/validate specs *only* via `sdd_*` tools (e.g. `sdd_initialize_system`, `sdd_validate_tree`).
  3. **No CLI Exec**: Do not run the `wairon` CLI (human tool). Use MCP tools `sdd_validate_tree` and `sdd_get_status` instead.
  4. **Delegation**: Delegate implementation via the `sdd-delegate` skill — live agent briefs (`sdd_get_agent_brief` MCP tool / `wairon-agent://` resource) are composed per call and always current; no session restart. Generated agent files are an optional materialized view of the same topology. User-owned per-agent guidance may live in `.wai/agents/<agent-id>.md` (folded into every brief; scaffold via `wairon agent customize <id>`).
  5. **Design First, then approval**: Complete the spec and pass `sdd_validate_tree`, then ask the human to approve it with `wairon lock` before writing code. Approval is the lock record (`.wai/lock.json`), not a spec's `status` — the lock does not rewrite statuses; `sdd_get_status` reports the approval state. Code linkage (`sourcePath`, `symbol`, `simPath`, …) is not part of the approval: declare planned source paths at design time, they cost no re-lock.
  6. **Prose is design; linkage is not**: an L4/L5 prose change — an implementation's or a method's description, intent or narrative step text — IS a design change: it re-opens the approval, so `wairon lock` is owed before code is implemented against it (an implement step asked for in the same turn as a prose edit waits for the human's re-lock; sequence spec turn → lock → code turn). Only code linkage is outside it.
  7. **Consistency**: Code must match L3 interfaces and L5 narratives exactly. If the spec is wrong, stop and update the spec.
  8. **Members & References**: A project may declare **members** in its `.wai/project.yaml` `members` (create one with `sdd_add_member`). A **part** (the default) stores some of this project's subsystems in another folder or repository: local ids, this project's lock. A **project** member is an independent boundary with its own spec tree and lock, designed from its own root. Reference what another project exports as `alias::name` (the alias is a member or a declared external, the name a public name of its L0 export table); an id without `::` is local. A leading `::`, `super::`, member paths and an L1 subsystem carrying `projectPath` are deprecated: they still resolve for one release, are reported, and `wairon doctor --fix` rewrites them.
  9. **Reachability**: every Portal verb is reached by a modelled caller or declared an entry (`invokedBy: { kind: entry }`) for real callers outside the design — never an entry invented to silence a finding.

### Code linkage facts
- **`injectedParams` are set when the code exists, by the implementer**: never declare them at design time. They name a parameter a framework imposes on written code (a request handle, a context) beside the contract's own; the implementer declares them with `sdd_write_narrative` (`injectedParams`) or `sdd_update_spec` once its code takes one, and removes a design-time guess its code does not take (`UNUSED_INJECTED_PARAM`) the same way — code linkage, no re-lock.
- **Plain JavaScript is checked too**: a JSDoc `@typedef` with `@property` lines declares a plain-JS shape, and in a binding module its field names are compared with the producer's type like a TypeScript interface's — add one rather than telling the human a `.js`/`.cjs` binding's fields cannot be checked.

### What the human runs — recommend these, never run them
The `wairon` CLI is the human developer's tool: you never run it, but when the human asks for something only it does, tell them the exact command:
- `wairon lock` — approve the design (then commit `.wai/lock.json`); `wairon lock-check` is the CI merge gate on that approval.
- `wairon validate --ci` — the CI gate. It FAILS on any error and on any warning, except the draft-related ones (a `DRAFT_*` warning, or an `UNUSED_COMPONENT` whose component is itself still draft or design status); notices never fail it, nor do the advisory live-externals findings. Say so plainly — no run is needed to know it.
- `wairon surface export --format openapi --portal <portal-id> --out <file>` — a Portal's OpenAPI document (one per Portal); `wairon surface diff` — the public-surface changelog since the last approval; `wairon export` — the whole resolved design as one JSON document.
- `wairon externals pin <alias>` — re-pin an external once its uses are adapted; `wairon externals status` — the live compatibility gate (exit 1 incompatible, 2 not compared).
- `wairon network declare` — declare this project's network boundary (your tool for it is `sdd_set_network`).
- `wairon member add | attach | detach | adopt | promote | demote | internalize | move | rename-alias | update` — the human's twins of the member tools; their `--report` is your `dryRun`.
- `wairon doctor --fix` — rewrite deprecated forms; `wairon generate` — refresh the generated guides, skills and context.
- `wairon agent customize <id>` — scaffold `.wai/agents/<id>.md`, guidance folded into every brief of that agent; `wairon agent brief <id>` — print a live brief.
- `wairon status` — readiness and approval; `wairon diagram` — architecture diagrams; `wairon network flows` — the allowed-flows matrix.
