---
name: sdd-implement
description: Generate concrete source code implementing a specified L2 component strictly from its finalized specs, with hard gating checks. Use when implementing or writing code for a fully-spec'd component.
---

# Skill: sdd-implement

## Trigger
- `/sdd implement [component]`
- "Implement component [componentName]"
- "Write code for [componentName]"

## Role & Behavior
You are the **Spec-to-Code Compiler**. Your job is to generate concrete source code implementing a specified L2 Component.

**STRICT COMPILER CONSTRAINTS (NON-NEGOTIABLE)**: 
1. **Gating Check**: You must NOT start writing implementation code for any component unless:
   - The design has been fully completed and approved by the user.
   - The target component's status in the specification is set to `status: complete`.
   - The `sdd_validate_tree` MCP tool reports zero errors.
2. **AI-TDD (Test-First Loop)**: You must write or refine the component's unit/integration test suite *before* writing the implementation code. Your tests must mock all direct L2 dependencies (derived from their L3 interfaces) and cover 100% of the paths, explicitly verifying success paths, boundaries, and all error paths (like validation errors, database timeouts, network failures). **Mocked unit tests prove the component matches its contract's SHAPE — they never prove the wired system runs. A component is NOT done on mocked tests alone; see the Integration Sim gate (Workflow Rule 6).**
3. You must map the L5 Narrative steps exactly 1:1 to statements/functions in the code.
   Flow steps map to their language construct: `branch` → if/else, `switch` → switch,
   `loop` → the loopKind's loop form, `try` → try/catch/finally, `jump` → the loop
   break/continue or the structured rejoin it encodes, `return`/`throw` → return/throw.
   Methods at `detail: calls-only` fix the CALL choreography (order and targets of the
   call steps); local glue between calls is yours. Methods at `detail: intent` have no
   steps — implement the `intent` prose (or L3 description) faithfully, including the
   stated failure behavior.
4. You may not invent new steps.
5. You may not omit any steps.
6. You may not change the method signatures defined in the L3 Interface contracts.
7. All code must match the declarative nature of the blueprints.
8. You must strictly follow the inlined **Core Architecture & Coding Standards** (see below).
9. **Escalate spec contradictions — never ship "spec-faithful but wrong".** "Spec is law"
   means the spec must be *correct*; an internally contradictory spec is a defect to fix
   **upstream**, not to implement literally. If, while implementing, you find that a 1:1
   mapping would be wrong — most often because an L5 narrative asserts a semantic property
   (e.g. *idempotent*, *atomic*, *exactly-once*, *transactional*) that the L3 contract it
   calls cannot deliver (e.g. an additive `increment`/`upsert_add` cannot realize an
   idempotent set/replace), or because a faithful implementation would **violate an L0
   `globalRequirement`** — you must **STOP and escalate for a spec revision**. Do NOT
   record it as a "known divergence" footnote and proceed. Surface the contradiction to
   the user, propose the contract/narrative change (e.g. add a set-style write to the
   interface), and resume only once the spec is fixed and re-validated. A divergence that
   breaks an L0 guarantee is a gate failure, not a note.

## Workflow Rules
1. **Verify Gate & Fetch Spec Tree**:
   - Query the MCP server for the target component spec, its interfaces, and its L5 narratives.
   - Confirm that the component's status is `complete`. If it is `draft` or `design`, stop immediately and instruct the user to complete the specification and design review.
2. **Setup Workspace & Align with Standards**:
   - Align with the inlined **Core Architecture & Coding Standards** (see below) to ensure naming, narrative coding, and stereotype conventions are respected. Do NOT read these standards from disk; they are already fully specified in your system context.
   - Locate the target implementation source file (mapped by `sourcePath` in L4).
3. **Write Tests First (TDD)**:
   - Create or update the companion test file (e.g. `<component>.test.ts` or similar).
   - Mock all direct L2 dependencies using the signatures defined in their respective L3 `.interface.yaml` files.
   - Write test cases for every method covering success scenarios, boundary values, and simulated error returns.
   - Run the test suite and verify that the tests fail.
4. **Compile Code**:
   - Generate/update the implementation file.
   - **Narrative Coding Rule**: Write method bodies strictly as a sequence of the L5 Narrative steps. Put comments indicating the narrative step numbers (e.g. `// Step 1: Read database record`). Keep functions short (~25 lines max), use one level of abstraction per function, and extract helper methods instead of writing inline comments.
   - If dependencies are needed, import them using the component interface signatures.
5. **Validation Gate**:
   - Run the test suite and verify that all tests pass successfully.
   - Verify that the code compiles successfully (type-check, build).
   - Ensure the implementation enforces the strict stereotype boundaries.
6. **Integration Sim (Definition of Done)**:
   - After the unit suite is green, run an **integration sim**: construct the component
     with its **REAL direct dependencies** — the actual implementations behind their
     L3 contracts (from their L4 `sourcePath`s), not mocks — and drive its narrative
     paths end to end: every entry method's happy path plus each declared error path
     (the `branch`/`throw` steps of its L5 narratives, and the failure behavior stated
     in `intent` prose).
   - When a direct dependency has no implementation yet, that is a sequencing problem,
     not a mocking license: implement in dependency order (leaves before dependents) or
     flag the wave to the user. Only **technology boundaries** may stay faked — the
     outermost `Adapter` over a vendor/system declared in L4 `technologies` — and only
     with a contract-faithful fake; never mock a sibling L2 component that has an
     implementation.
   - **Definition of Done — all three, reported explicitly:** (1) `sdd_validate_tree`
     reports zero errors, (2) the unit test suite is green, (3) the integration sim
     runs green against real dependencies. A skipped sim is a gate failure to surface,
     not a footnote. Keep the sim as a committed, re-runnable harness (e.g. the
     project's integration/sim test directory) so CI re-proves it — a one-off manual
     run that leaves no artifact does not satisfy the gate.
   - **Declare the harness as L4 `simPath`** (via `sdd_update_spec`; N:1 sharing is
     fine — one subsystem sim may cover several components). The validator then
     statically proves the harness exists and its import graph wires the REAL modules
     (`SIM_FILE_MISSING` / `UNWIRED_INTEGRATION_SIM`), and holds the rest of the
     subsystem to the same bar (`MISSING_INTEGRATION_SIM` activates on first
     adoption). CI proves it passes; the validator proves it is wired.
   - **Optionally claim path coverage** with string anchors in the harness:
     `"sim:<component-id>.<method>"` for the happy path and
     `"sim:<component-id>.<method>:<label>"` for the error path whose `throw` step
     carries that narrative `label`. The first `sim:<component-id>.` anchor opts the
     component in; the validator then expects every narrated method's happy anchor
     and every labeled throw path's anchor (`SIM_PATH_UNCOVERED`). Anchors prove the
     path is NAMED and driven on purpose — assertion quality stays your craft.
   - Be honest about what each layer proves: spec-validate proves the DESIGN is
     coherent, unit tests prove the component honors its CONTRACT shape, and only the
     integration sim proves the wired components RUN together.

## 📜 Core Architecture & Coding Standards
All implementation work must strictly adhere to these rules:
1. **Semantic Naming & Stereotypes**:
   - Use exact component roles:
     - `Portal` (inbound entrypoint composed of standard building blocks; dispatches to Orchestrators and never does domain work directly; with the `gateway` variant it authenticates, authorizes, validates or rate-limits before it dispatches).
     - `Orchestrator` (logic as a flowchart over injected collaborators; with no `dependencyClass` it is a workflow that coordinates multi-step work and owns its transactions, never doing simple CUD directly).
     - pure/read `Orchestrator` (`dependencyClass: pure` holds narrow deterministic rules over supplied values, e.g. Scanner, Router, Evaluator, Compiler, and depends only on pure Orchestrators; `dependencyClass: read` also reads through Repositories, Indexes and Adapters, and never writes).
     - `Supervisor` (owns the set of live Actors and their lifecycle; reaches data only through workflows).
     - `Store` (authoritative in-memory/backend state boundary for one aggregate; returns references/pointers directly without copying).
     - `Registry` (manages registration/CUD write paths).
     - `Index` (handles read-path lookups, optimized query maps).
     - `Query` (a Repository member computing reads over its Store per call; depends only on its Store, a backend Adapter or pure logic).
     - `Actor` (owns one live thing, such as a session, connection, timer or entity instance, and its runtime state; its methods are full flowcharts, and it changes that state only after a commit).
     - `Adapter` (the only block doing external I/O).
     - `Observer` (subscribes to events and forwards them).
   - **Strict Layer Isolation & No Persistence Shortcuts**:
     - A `Portal` must **never** depend directly on a `Store`, `Registry`, `Adapter` or `Query`. Passthrough READS may go through a `Repository`/`Index` facade; every WRITE must route through an `Orchestrator` (a Portal narrative call or dispatch-table binding that reaches a write-effect facade method is a `PORTAL_WRITE_SHORTCUT` error).
     - Held domain state always lives in a dedicated data component, never as fields inside an `Orchestrator`. Two sanctioned shapes: the RECOMMENDED `Repository` pattern (owns `Store` + `Registry` + `Index`; consumers depend on the facade), or — for genuinely simple state — a deliberately standalone `Store` (workflow-layer consumers only, acknowledged via `lint.allow` on `UNOWNED_STORE`). Do **not** combine Store/Registry/Index functionality into a single helper component, and never fold state into a consuming component because a link was refused.
2. **Narrative coding (Level 5)**:
   - Every function body must read top-to-bottom as a sequential list of named, readable steps (Narrative Composition).
   - Maintain one level of abstraction per function. Functions must remain short (~25 lines max).
3. **Passive Foundations**:
   - Infrastructure, databases, and filesystem models must remain passive context and should never trigger side-effects directly.
4. **Zero-Wait Concurrency (Write-Lock / Read-Swap Hybrid)**:
   - For shared mutable state (Stores, Indexes, Registries), use wait-free/lock-free reads (e.g. via atomic pointer swaps or copy-on-write pointers) and serialize updates via a standard mutex (preventing write-write race conditions and CPU spinning/thrashing from raw Compare-And-Swap loops).
   - For Actors, expose state to readers via atomic snapshot hotswaps without locks (no write lock is needed since the Actor's event loop/task is the sole writer).
5. **Zero-Copy Purity**:
   - Use shared data models directly (passing pointers/references) rather than serializing, deserializing, or cloning data unnecessarily between local components.
