---
name: sdd-narrative
description: Draft the precise step-by-step execution narrative (L5) for a specific implementation method, updating .wai/phased_design.md Stage 5. Use when designing or writing the narrative/flow for a method or component.
---

# Skill: sdd-narrative

## Trigger
- `/sdd narrative [component]`
- "Let's design the [methodName] method"
- "Let's write a narrative for [methodName]"

## Role & Behavior
You are the **Method Designer**. Your job is to draft the precise step-by-step narrative for a specific implementation method. 

You must read, respect, and update `.wai/phased_design.md` (specifically Stage 5: Execution Flow Narratives).

**Strict Rule**: No code writing. Write only structured narratives (L5 specs) composed of sequential, named logical steps.

## Workflow Rules
1. **Identify Intent**:
   - Ask the user for the high-level intent, signature, and contract of the method.
2. **Choose the detail level FIRST** (the narrative detail dial):
   - `full` — a step-by-step narrative, with flow structure where the logic branches. Default for Orchestrators, Supervisors, Actors, and patterns.
   - `calls-only` — only the cross-component `call` choreography. Default for Portals, Observers, and Adapters (boundary pass-throughs: real logic belongs in the Orchestrator they forward to — if a Portal method needs branching, that is a smell).
   - `intent` — no steps; instead write an `intent` paragraph stating what the method does and how it fails. Default for Stores, Indexes, Queries, and Registries. The validator enforces an intent floor: placeholder-thin prose is rejected.
   - Omit `detail` when the stereotype default already matches; declare it (per method or spec-level) only to override. Levels are floors — extra detail is never penalized.
3. **Draft Narrative Steps** (for `full` / `calls-only`):
   - Narratives are a FLAT ordered list; the order mimics the code lines. Flow structure jumps by step number — blocks are just skipped regions.
   - Step types:
     - `local`: internal logic (calculations, state mapping).
     - `call`: call to another component (`targetComponent` + `targetMethod`).
     - `dispatch`: a capability routed through a generic-dispatch Portal's dispatch table (`targetComponent` = the Portal, `capability` = the routed name). Use this instead of a bare `call` to the portal's generic handle — the gate validates the capability against the portal's table (`UNSERVED_CAPABILITY`) and reachability follows the bound server.
     - `register`: a runtime-callback handoff (`targetComponent` + `targetMethod`, same shape as `call`): this method hands the target method to the runtime (timer, event listener, shutdown hook) to invoke LATER. Reachability follows the edge (the callback is reached wherever the registrar is), but it is never an invocation — exempt from call-graph conformance, call-cycle detection, and the durability boot walk. If the real caller lives entirely outside the modeled graph, declare `invokedBy` on the target's L3 method instead.
     - `branch`: if/else — `condition` + `onFalseStep` (true continues at `onTrueStep` or the next step). Chain else-ifs by targeting another branch step.
     - `switch`: `cases: [{value, step}]` (required) + optional `on` (names the value dispatched on) + optional `defaultStep` (unmatched values continue at `defaultStep`, or the next step).
     - `loop`: header step; body = next step through `endStep`. `loopKind: forEach | for | while | doWhile` with `over` (forEach/for) or `condition` (while/doWhile).
     - `try`: guarded region (body = next through `endStep`) with `catches: [{error, step}]` and optional `finallyStep`.
     - `parallel`: concurrent fan-out/join — body = next through `endStep`, `branches: [{step}]` (≥2, ascending, first = the step right after the header) name the arm entries; arms are contiguous sub-regions and flow continues after `endStep` once ALL arms complete. Use for genuinely concurrent work (scatter-gather, sensor fan-in) — not as a stylistic grouping.
     - `jump`: unconditional goto (`toStep`) — how a loop breaks/continues and how a catch block rejoins the main flow (put one at the end of a try body to skip the handlers).
       - Continue a loop in one of two ways: a `jump` whose `toStep` is the loop header, or a `branch` whose false path (`onFalseStep`) targets the loop header ("not done yet: go round again"). Both are continues to an enclosing loop header, which the flow lint exempts from `BACKWARD_JUMP`.
       - In nested loops, give each loop its own closing step. A jump from the outer loop's body must land on a closing step of the outer level, never on a step that closes several nested loops at once: that step lies inside the inner loop's region, so jumping to it from outside is `JUMP_INTO_REGION`.
     - A `call`/`dispatch` step may set `detach: true` — fire-and-forget: the call is issued and the narrative continues without awaiting the result. Only detach when no later step consumes the result and failure handling genuinely lives with the callee.
     - `return`: terminator (optional `outcome`); `throw`: error terminator (optional `error`).
   - `stepNumber` may be omitted in `sdd_write_narrative` — it defaults to the 1-based array position; jump fields reference those numbers. `sdd_update_spec` inserts/deletes renumber AND relocate all jump fields automatically.
   - **Prefer symbolic labels over hand-counted step numbers.** Give a target step a `label` (e.g. `label: retry`) and reference it with the jump field's `*Label` twin — `toLabel`, `onTrueLabel`, `onFalseLabel`, `defaultLabel`, `endLabel`, `finallyLabel`, and `label` inside `cases`/`catches`/`branches` entries. Labels resolve to step numbers at write time (the stored spec keeps plain numbers); an unknown label REJECTS the write instead of silently mis-jumping, and an `sdd_update_spec` delta may reference labels anchored on pre-existing steps.
   - Error paths belong in the SAME narrative (the flowchart renderer visually separates them and can hide them) — never write separate happy/unhappy narratives.
4. **Verify Contracts & Boundaries (MCP)**:
   - For every `call` step, query the MCP server to verify that the target component is declared in the calling component's dependencies and that the target method exists on its L3 interfaces.
   - Run `sdd_validate_tree` to ensure this narrative doesn't create circular dependencies or break component type boundaries.
   - **Verify asserted semantics against the contract.** If a step claims a semantic
     property — *idempotent*, *atomic*, *transactional*, *exactly-once* — the target L3
     method MUST declare it in its `guarantees` list, and its shape must actually deliver
     it. An **additive** write (`increment`, `upsert_add`, "add amount to…") cannot realize
     an *idempotent* update; a reconcile/rollup that must be idempotent needs a
     **set/replace** method declaring `guarantees: [idempotent]`. The gate enforces this
     *consistency* (a narrative claim with no matching contract guarantee is flagged
     `NARRATIVE_SEMANTIC_UNBACKED`) but cannot verify the guarantee is truly delivered —
     that is on you and the implementer. If the contract lacks the needed method or
     guarantee, revise the L3 interface first (mandatory when an L0 `globalRequirement`
     depends on it).
5. **Register & Promote**:
   - Present the drafted narrative content (the exact step-by-step YAML structure) and a concise summary of the key flow/design choices directly in the chat message to the user. Do NOT create temporary/intermediate markdown review files in the brain or workspace for this feedback loop.
   - Upon user approval, call `sdd_write_narrative` to save it in the spec tree.
   - Once the interface, narrative, and spec for this component compile without errors, recommend changing the component's status field to `status: complete`.
   - Update Stage 5 checkboxes in `.wai/phased_design.md`.
