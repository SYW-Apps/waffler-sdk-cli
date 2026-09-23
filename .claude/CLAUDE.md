

<!-- wairon-guide-start -->
<!-- wairon-version: 5.1.1-dev.23 -->
## Wairon — Spec-Driven Development (you are operating inside it)

This project uses **wairon**. System specs live under `.wai/specs/` (L0 System → L1 Subsystem → L2 Component → L3 Interface → L4 Implementation → Narrative); agent topology and code are derived from it.

**Do NOT search files or read agent configs to learn about wairon or SDD. Use the context here and the `sdd-architect` skill to start.**

**Your first move: call the `sdd_get_status` MCP tool** (or `wairon/sdd_get_status`) to see the spec tree. Do not parse files or run CLI commands manually.

### How you operate
- **To design/modify specs**: Use **`sdd-architect`** skill (in `.claude/skills/` or `.gemini/skills/`).
- **Manage specs via MCP tools only**: Use `sdd_initialize_system`, `sdd_add_subsystem`, `sdd_add_component`, `sdd_define_interface`, `sdd_write_narrative`, `sdd_add_type`, `sdd_get_spec`, `sdd_delete_spec`, `sdd_validate_tree`, and `sdd_get_status` (namespaced if needed). Do not edit specs manually.
- **Subprojects & Namespacing (Chaining)**: If a subsystem defines a `projectPath`, its entire `.wai/` spec tree is recursively loaded and namespaced with the subsystem ID as a prefix (using `::`, e.g. `billing::invoice::invoice_portal`). Use the qualified namespaced ID with the parent MCP tools; wairon will resolve the path and strip the prefix automatically.
  - **Leading `::`**: Bypasses the local subsystem prefix to resolve absolute from the system root (e.g. `::shared::error-type`).
  - **`super::`**: Goes up one parent subsystem level (e.g. `super::sibling_comp`, `super::super::parent_sibling`).
- **Do not run the `wairon` CLI**: Use `sdd_validate_tree` and `sdd_get_status` instead of CLI commands.
- **Handoff to implementation**: Once design is complete and validates cleanly, tell the human: *"The specs are complete and validate. Please run `wairon lock` to confirm and freeze them."* No session restart is needed after the lock — delegate implementation right away via the `sdd-delegate` skill.
- **To implement code**: Delegate via the `sdd-delegate` skill: fetch the component's live brief with the `sdd_get_agent_brief` MCP tool (or the `wairon-agent://` resource) and spawn a subagent from it. Briefs are composed per call from the current spec tree, so they are always current — never wait for a restart. Implementations must match L3 interfaces and L5 narratives exactly. Generated agent files under `.claude/agents/` are an optional materialized view of the same topology — the live briefs are canonical.

### Rules (enforced by `sdd_validate_tree`)
1. **Design before code**: Complete spec and pass validator before writing source code.
2. **Human-in-the-loop**: Ask user approval for each spec layer before proceeding.
3. **Spec consistency**: If a 1:1 narrative match is incorrect or conflicts with L0 requirements, escalate a spec revision first. Never ship mismatched code.
4. **No persistence shortcuts & strict layers**: A Portal must never depend directly on a Store, Registry, or Adapter. Portal reads MAY go through a Repository/Index facade (passthrough reads need no per-entity Orchestrator ceremony), but a Portal narrative call or dispatch-table binding that reaches a write-effect facade method is an error (`PORTAL_WRITE_SHORTCUT`) — writes always route through an Orchestrator. Held domain state always lives in a dedicated data component, never as fields inside an Orchestrator. Two sanctioned shapes: the RECOMMENDED Repository pattern (owns Store + Registry + Index; consumers depend on the facade), or — for genuinely simple state — a deliberately standalone Store (workflow-layer consumers only, acknowledged with a lint.allow reason on the UNOWNED_STORE warning). Never combine Store/Registry/Index roles into one component, and never fold state into a consuming component because a link was refused.

### Component Vocabulary
* **Blocks**: Portal, Orchestrator, Supervisor, Actor, Store, Index, Query, Registry, Adapter, Observer.
* **Patterns**: Repository — the composable pattern that `owns` its member blocks: one Store with its Registry, Indexes and Queries, and optionally an Adapter.
* **Variants**: `gateway`, a Portal that authenticates, authorizes, validates or rate-limits before it dispatches. Logic is an Orchestrator, and `dependencyClass: pure | read` bounds what it may depend on (unset = a workflow). Specialist and Gateway are retired stereotypes.
* Use `owns` for private member containment (exactly one hop) and `dependsOn` for collaborators. Never use generic suffixes like "Manager", "Helper", or "Utils".
<!-- wairon-guide-end -->
