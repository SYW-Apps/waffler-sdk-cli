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
   - The design is **approved**: the human ran `wairon lock`, and `sdd_get_status` reports the approval ("Approved: <date> by <who>") with none of the target component's specs listed as changed since. (`wairon lock-check` is the same verdict in CI.)
   - The `sdd_validate_tree` MCP tool reports zero errors.
   - Do NOT gate on a spec's `status` field. `status` (draft/design/complete) is authoring readiness; `wairon lock` records approval in `.wai/lock.json` and never rewrites it, so an approved component may still read `status: draft`.
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
6. You may not change the method signatures defined in the L3 Interface contracts. Contracts spell types in wairon's neutral grammar (`list<T>`, `map<K, V>`, `T?`, `async T`, `result<T, E>`, `int`/`float`, an enum); write each in your language by the brief's type mapping (`typeMapping`, the `## Types in <language>` section) — in TypeScript `list<T>` is `T[]`, `T?` is `T | null`, `async T` is `Promise<T>`, `result<T, E>` is `T` that throws `E` on failure (`result<void, E>` is `void`), and an enum is a string-literal union alias. Annotate each realizing function's return with the contract's `returns` (the gate compares them: `RETURN_TYPE_MISMATCH`); in Python, name parameters in snake_case (the contract's `documentId` is `document_id`) and annotate them and the return — the gate reads `def` signatures too.
   - Add no public surface the contract does not declare: a helper method is `private` (a leading underscore in Python), a module helper is not exported unless something uses it. A public method or exported function no contract declares and nothing uses is `UNDECLARED_PUBLIC_MEMBER`.
   - Every call your function makes, in its own body, to a collaborator its narrative names is a narrative step (`UNNARRATED_CALL` otherwise); a Portal served by a router names it in the implementation's `router:` (`ROUTES_UNCHECKED` otherwise).
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
   - Confirm the design is approved: `sdd_get_status` reports "Approved: …" and does not list any of this component's specs as changed since the approval. If there is no approval on record, or this component's specs changed after it, stop immediately and ask the human to review the design and run `wairon lock` (never run it yourself).
2. **Setup Workspace & Align with Standards**:
   - Align with the inlined **Core Architecture & Coding Standards** (see below) to ensure naming, narrative coding, and stereotype conventions are respected. Do NOT read these standards from disk; they are already fully specified in your system context.
   - Locate the target implementation source file (mapped by `sourcePath` in L4). A file the brief marks `(planned — create it)` does not exist yet: create it at exactly that path — the fence already holds it. The fence (`codeFence`) is exact files: write code only there, tests beside them. A type the brief fences is yours to create at its planned home; a shared type file (`sharedPaths`) is created there too, exactly as its spec declares it — never redeclare a type in your own file. Shared files, and files no spec names (a helper, the composition root, test setup, a module manifest so the tests run in place), may be created or extended only for this component's needs; name each one in your report. One that would need a responsibility of its own (domain logic, held state, a decision the design does not make) is a design change: stop and report it. Never delete or move a file or folder outside your fence — an ignored or scratch-looking project folder included — and never use an existing project folder as scratch space: keep scratch work in a temporary directory outside the repository. A named file not written yet is only `SOURCE_FILE_PLANNED` (a notice), at method level as at implementation level; once any file a component names exists, a contract method naming no file is `METHOD_SOURCE_PATH_MISSING` and one its existing file does not hold is `UNREALIZED_METHOD`.
   - **Code linkage is not approval.** `sourcePath`, `symbol`, `exportedVia`, `simPath`, `injectedParams` and conformance tiers say where the design is realized, not what it is: they are outside the approved digests. When the implementation names no `sourcePath` yet, or names the wrong file, set it with `sdd_update_spec` — it costs no re-lock and never turns the approval stale.
   - **Externals.** A contract you consume from another project is judged against its pin. If `sdd_validate_tree` reports an ADVISORY `EXTERNAL_LIVE_INCOMPATIBLE` (a used member changed, was renamed — the new name is given — or is gone in the live producer), adapt the uses, then ask the human to re-pin (`wairon externals pin <alias>`). Advisory findings never fail the gate; they are not a reason to stop, nor to code against the live producer's new shape without adapting the spec first. When your code reaches a producer through a hand-written binding module (a typed binding to a native library, a client stub), name it on the implementation as `bindings` (code linkage, no re-lock) and write the producer's names in it exactly as the pin has them: `validate` compares the binding with the pin and reports a stale name, parameter or field as `BINDING_DRIFT`, with the rename to follow. Tag each declaration in the binding that mirrors a producer name with a doc comment naming it — `/** alias::name */` above an interface, a class or a type, `/** alias::name.method */` above a free function — so `validate` compares it with that export alone and names a verb the producer no longer holds; an untagged declaration nothing matches by name is read as the binding's own plumbing and never judged.
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

## 🧭 Working conventions (what each one cost)

These are not house style. Each one is here because a delegated change went wrong
without it, and a convention whose reason you can see is one you can still apply
to the case nobody wrote down.

1. **Specs change through the validated write path — never a text edit.**
   The `sdd_*` tools (or, in-process, the library's own write function) are what
   renumber narrative steps, relocate jump targets, and refuse a delta the schema
   does not accept. Hand-editing a file under `.wai/` skips all three, and the
   damage surfaces later in somebody else's validate run. If a running server
   cannot express a field your change introduces, that is a reason to restart it
   or call the library directly — never a licence to open the editor.
2. **Read every write back from disk before you build on it.**
   A write's answer is what the *server* believes. `sdd_update_spec` returns a
   structured change report naming what actually moved — read it, because
   "nothing changed" and "everything changed" are different answers that used to
   be the same sentence. When the build on disk moved after the server started,
   every answer carries `staleServer: true` and a ⚠ STALE SERVER banner
   (`sdd_get_status` leads with it). If the rebuild changed a spec or tool
   schema, the server REFUSES every spec write and writes nothing
   (`writesRefused: true`), because a stale process once silently replaced an
   entire `params` list while reporting success; if only the build changed,
   writes still go through under the warning. Either way only the human can
   clear it, by reconnecting the server (`/mcp reconnect wairon`) — ask for
   that, finish what needs no write, and list the spec writes still owed. Open
   the file after every write: the report is evidence, the file is truth.
3. **The lock is the human's signature, not a step in your task.**
   Never run `wairon lock`. Your work ends at "the tree validates" — say so and
   hand it over (`sdd-architect` carries the handoff wording). Locking on the
   human's behalf forges the one record that says a person looked.
4. **Measure before you repair.**
   When a change lights up a large number of findings, report the count and stop.
   Whether to fix them, carry them, or scope them out is the maintainer's call,
   and it is cheap to ask before the work and expensive after. Separate *your*
   breakage from debt that was already there before you report either number: a
   wave that mixed the two spent its effort across 362 findings and could only
   honestly claim 224 of them.
5. **Prove a behaviour by revert — and restore from your own snapshot.**
   Copy the file aside, overwrite it, run the thing, then restore *from the copy*.
   Never `git checkout --` to undo the experiment: that restores the *committed*
   version, so every uncommitted change in that file — yours and anyone else's —
   dies with the proof. It has already cost about 120 lines of work that nobody
   could get back.
6. **Delete the temporary harness before you commit, and say that you did.**
   A scratch script left behind reads as a deliverable to the next person and
   quietly becomes a file somebody now maintains. (An integration sim is the
   opposite case — it is *meant* to stay, committed and declared as `simPath`.)
7. **A refusal with reasoning is a result.**
   If the code contradicts the premise you were handed, say so and show the
   measurement. Building what was asked on a premise you have already disproved
   spends the work twice and buries the finding.
8. **Never declare what the code does not do.**
   A `lint.allow`, a `simPath`, a coverage anchor, or a `status: complete` that
   silences a finding without the behaviour behind it is worse than the finding:
   it moves a known defect out of a list somebody reads and into a claim somebody
   trusts.
9. **Report what you did not do as carefully as what you did.**
   The gate you skipped, the path you left untested, the thing you could not
   reproduce — that is what the next person needs. A report listing only
   successes gets read as complete.

## 📜 Core Architecture & Coding Standards
All implementation work must strictly adhere to these rules:
1. **Semantic Naming & Stereotypes**:
   - Use exact component roles:
     - `Portal` (inbound entrypoint composed of standard building blocks; dispatches to Orchestrators and never does domain work directly; with the `gateway` variant it authenticates, authorizes, validates or rate-limits before it dispatches).
     - `Orchestrator` (logic as a flowchart over injected collaborators; with no `dependencyClass` it is a workflow that coordinates multi-step work and owns its transactions, never doing simple CUD directly).
     - pure/read `Orchestrator` (`dependencyClass: pure` holds narrow deterministic rules over supplied values, e.g. Scanner, Router, Evaluator, Compiler, and depends only on pure Orchestrators; `dependencyClass: read` also reads through Repositories, Indexes and Adapters, and never writes).
     - `Supervisor` (owns the set of live Actors and their lifecycle, and may own its supervision state — a Store or Registry of its own; reaches shared data only through read- and lifecycle-effect methods, and writes it through a workflow).
     - `Store` (authoritative in-memory/backend state boundary for one aggregate; returns references/pointers directly without copying).
     - `Registry` (manages registration/CUD write paths).
     - `Index` (handles read-path lookups, optimized query maps).
     - `Query` (a Repository member computing reads over its Store per call; depends only on its Store, a backend Adapter or pure logic).
     - `Actor` (owns one live thing, such as a session, connection, timer or entity instance, and its runtime state; its methods are full flowcharts, and it changes that state only after a commit).
     - `Adapter` (the only block doing external I/O).
     - `Observer` (subscribes to events and forwards them).
   - **Reach, transports and libraries**:
     - A Portal's verbs are reached by the callers the design models or by the outside callers its `invokedBy` entry names. Serve exactly those: the Portal's L4 `router` is the entry its file exports for whatever process hosts it. Never add a verb, route or command the contract does not declare, and never ask for an entry to make an `UNUSED_*` finding go away — report the unreached verb instead.
     - Another project's library (an `InProcess` Portal) is imported and called directly from the component the narrative names — no wrapper Adapter unless the design has one. Keep pure and read logic to the library verbs whose `effect` the design allows.
     - Function names follow the contract's method names in the language's casing; a wire name that differs (an HTTP path, a CLI command, a JSON-RPC method) belongs to the endpoint binding — route it there, do not rename the function after it.
   - **Handler shape — the green path for a framework handler** (no design change, no re-lock):
     - A verb's function takes the contract's OWN parameters, in order, under the contract's names. `cancelOrder(req, res)` realizing `cancelOrder(orderId, customerId)` is reported as two substitutions (`UNREALIZED_PARAM` + `UNDECLARED_PARAM`), annotated or not; so is a request object, or any object sharing none of the record's fields, where the contract declares a record.
     - What a framework hands the function besides those — a context, a request, a response, `next` — is wiring: name it in the implementation's `injectedParams` with `sdd_update_spec` (code linkage, outside the approval). It is matched with or without its leading underscore, at the START or the END of the list: `handler(ctx, req, orderId, customerId, res)` with `injectedParams: [ctx, req, res]` is green.
     - Two honest shapes: (a) the router unpacks the path, query and body and calls `cancelOrder(orderId, customerId)` (handles injected around them if the framework insists); (b) the framework hands the handler the request alone — inject it and read each contract parameter off it **by its own name** (`req.params.orderId`, `req.query['limit']`, `const { orderId } = req.params`, or a helper's result: `const { orderId } = pathParams(req)`), which realizes the parameter through the handle. The one record parameter is realized by the body read whole: `req.body`, or — plain node:http has no `req.body` — the request handed to a body helper whose result you bind and use whole (`const cart = await readJsonBody(req)`, then pass `cart` on). A result only destructured or only read for fields is not the record, and a contract parameter never read is still `UNREALIZED_PARAM`.
     - An injection no handler takes is `UNUSED_INJECTED_PARAM` — drop it. A leading `_` marks a parameter the body provably never reads; a used `_secret` is judged like any other.
   - **`injectedParams` are yours to set, when the code exists** (any component, not only a Portal): declare one only once your code takes a parameter its framework or wiring imposes beside the contract's own — with `sdd_write_narrative` (`injectedParams`) or `sdd_update_spec` (`{"injectedParams": ["ctx"]}`). A dependency the code holds as a field or receives in a constructor is not one. A list already on the implementation may be a guess written at design time, before any code existed (`db`, `deps`, `config`, `req, res`): keep the names your functions really take and remove the rest — `sdd_update_spec` with `{"injectedParams": [{"value": "db", "action": "delete"}]}`, or `[]` to clear it. It is code linkage, so neither change asks for a re-lock; the brief's **Code linkage you own** section lists what is declared now.
     - `conformance: off` (on the implementation or one method) is for code wairon is not meant to compare with its contract — generated or vendored code. It switches the realization checks off (method, parameters, async, forward call claims), never the doctrine: an unnarrated write, a write shortcut and a call the analysis cannot follow are still reported.
   - **Routers**: set the Portal implementation's `router` (code linkage) to the table or function the server actually dispatches — the entry its file exports (`handleOrderRequest`), a class's method as `Class.method` (`HabitPortal.handle`, the class exported), a central module's table or entry (`src/routes.ts#routes`), or the module alone (`src/routes.ts`). Where the server dispatches an aggregate spreading every Portal's table (`routes = [...ingestRoutes, ...statsRoutes]`), name the aggregate, not the Portal's own table: a table left out of the spread would read clean (`ROUTER_NOT_DISPATCHED`, a notice, is the only signal). A module-private table or function cannot be named: export it. Write routes in a readable idiom: guards on `<request>.method` and `parts[i]`, or a route table — an array of `{ method, path }` objects or an object keyed `VERB /path` (template-literal keys over constants, and a table returned in place, are read). Table paths are written under the prefix the router strips, and that prefix must be the Portal's `basePath`.
   - **Another project's code** is reached through what it exports — its member alias or a pinned external — or through a binding module the implementation declares in `bindings`. Importing a name straight out of its source tree that it does not export is `CROSS_PROJECT_SOURCE_IMPORT`.
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
