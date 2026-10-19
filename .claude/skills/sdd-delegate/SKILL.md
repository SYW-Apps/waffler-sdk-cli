---
name: sdd-delegate
description: Delegate scoped implementation work by fetching a live agent brief (sdd_get_agent_brief / wairon-agent://) and spawning a generic subagent from it. Use when handing a component or subsystem task to a focused sub-session.
---

# Skill: sdd-delegate

## Trigger
- `/sdd delegate [agent]`
- "Delegate [component] to its implementer"
- "Hand this off to the [subsystem] owner"

## Role & Behavior
You are the **Delegation Orchestrator**. Your job is to hand scoped work to a focused subagent built from a LIVE agent brief — never from a generated per-component agent file. The flow is hierarchical: the main session delegates to owners, and an owner subagent may delegate further down using this same skill.

**Why live briefs**: a brief is composed from the CURRENT spec tree on every call. A re-lock changes the next fetch's result — sessions never restart to pick up topology changes. Component-level delegation therefore has NO generated files; per-subsystem owner files are the only agent files wairon can generate, and only when the project opts in (`rules.materializeAgentFiles`) — an optional materialized view of the same briefs.

**Project guidance**: a project may carry user-owned per-agent guidance in `.wai/agents/<agent-id>.md` — it is folded into every brief under `## Project guidance` (scaffold one with `wairon agent customize <id>`).

## Workflow Rules
0. **Implementation waits for approval**: before delegating implementation work, call `sdd_get_status` and confirm it reports the design approved ("Approved: <date> by <who>") with none of the target's specs listed as changed since. Approval is the human's `wairon lock` record, not a spec's `status` — the lock never rewrites statuses, so do not wait for `status: complete`. If there is no approval, or the target changed after it, stop and ask the human to review and run `wairon lock`. (Design or investigation tasks need no approval.)
1. **Discover the target agent**:
   - Call the `listAgents` MCP tool, or list MCP resources and look for `wairon-agent://<agentId>` entries.
   - Pick the agent whose `ownedPaths`/domain matches the task. If no agent fits, stop and tell the user the topology has a gap.
2. **Fetch the LIVE brief**:
   - Call `sdd_get_agent_brief(agentId)` (or read the `wairon-agent://<agentId>` resource).
   - The brief carries: `agentId`, `name`, `template`, `domainRoot?`, `ownedPaths`, `readPaths?`, `instructions`, `variantGuidance?`, `typeMapping?` (how the contracts' neutral types are spelled in the language the agent's code is written in, also folded into `instructions` under `## Types in <language>`; TypeScript's table is the one code conformance reads back, while Rust's and Python's are mapping-only tables the code analyzer does not check, and the table says so), `codeFence?` (where the agent may write code: exactly the files its specs alone name — its implementation, method, simPath and binding files and the files of the types it owns — planned ones included, never a folder glob, so parallel fences never overlap), `sharedPaths?` (files it may touch but does not own: files other components' specs name too, such as a shared type module, and the module setup on the way to its code — manifests, compiler settings, package roots, a planned one included (a Portal's `mod.rs`, where its sibling adds only the line declaring its module) — plus unnamed helpers beside it), an `## Externals used` section for a component reaching another project (each alias, the names it uses, the producer's transport and abi where the pin records them, the pinned snapshot `.wai/externals/<alias>.yaml`, which joins `readPaths` — code against the pin, never the producer's source — or, for a member project, its L0 export table read live, with the names spelled as that table exports them; and the binding modules its implementations name), a `## Handler shape` section for a Portal (the contract's own parameters per verb, the handles named in `injectedParams`, the `router` linkage), a `## Code linkage you own` section (when to declare `injectedParams`, and the ones declared now, which may be design-time guesses to remove), and — when the project opted into `execution.tier` — `profile` and `budget`.
   - **Never reuse a brief across delegations or after a re-lock** — fetch fresh per delegation; the call is cheap and the brief is always current.
3. **Spawn a GENERIC subagent from the brief**:
   - Prompt: `brief.instructions`, plus the concrete task description.
   - Write fence: the subagent writes code in the files `brief.codeFence` lists (and tests beside them), its spec files in `brief.ownedPaths`. The fence is exact files, existing or planned; a planned file is marked `(planned — create it)` and the subagent creates it there — including the file of a type it owns, never a local copy of the type. When the brief names no code location, declare the planned `sourcePath` first (`sdd_update_spec` on the implementation) — code linkage is not part of the approval, so this costs no re-lock — and fetch the brief again. Fences of sibling components never overlap, so their subagents can run in parallel without you narrowing anything. The fence bounds deletions too: the subagent never deletes or moves a file or folder outside it (an ignored project folder included) and keeps scratch work in a temporary directory outside the repository, never in an existing project folder.
   - Shared files: `brief.sharedPaths`, and any file no spec names (a shared helper, the composition root, test setup, a module manifest beside the code), may be created or extended only for this component's needs — a module setting so its tests run in place, a wiring line, a shared type at its planned home exactly as its spec declares it — and the report names each one touched. A shared or unnamed file that would need a responsibility of its own (domain logic, held state, a decision the design does not make) is a design change: the subagent stops and reports it. When siblings run in parallel, give each shared file one writer (or sequence the waves) — the brief cannot know which sibling runs first.
   - Required first reading: `brief.readPaths` — the subagent reads these before any edit.
   - Pass `brief.variantGuidance` along when present, and `brief.typeMapping` — the subagent writes `list<T>`, `T?`, `async T`, `result<T, E>` and an enum by that mapping, never by guess.
4. **Apply `brief.budget` when it is present** — constituting the subagent correctly is part of spawning it, not a separate concern. When the brief carries no budget the project has not opted in; spawn as you otherwise would.
   - `modelTier` → your host's model families. On Claude Code: `small`→haiku, `standard`→sonnet, `large`→opus, `frontier`→fable. A host that cannot select models ignores this rather than approximating it.
   - `effort`, `maxTurns` → pass through where the host supports them. The turn ceiling is a circuit breaker: hitting it means the task was scoped too big, so re-scope and re-delegate rather than raising it.
   - `toolClass` → `read-only` grants read/search only; `implement` adds edit/write/shell; `orchestrate` is for a router that owns nothing and must not read bulk content.
   - `allowNestedDelegation: false` → withhold the delegation tool entirely, so the subagent does its own work instead of spawning another layer.
   - `mcp: none` → do not load MCP servers into the subagent; its brief already quotes the contract it needs.
   - **`frontier` is never an owner tier.** Derivation never assigns it. Treat it as a sparring partner: when a subagent is genuinely stuck on something the specs do not settle, escalate that *question* to a frontier-tier helper and bring the answer back. A component whose owner truly needs frontier capability to operate is usually a component doing too much — raise that as a spec concern rather than spending the tier.
   - Fan delegations out in ONE message when they are independent. Siblings spawned together share a cached prompt prefix; dispatched one at a time they each pay for it.
5. **The subagent does the work**:
   - For component implementation it follows the `sdd-implement` skill (gating checks, AI-TDD, narrative coding) and reports back: what changed, test results, anything out of scope.
6. **Review & integrate**:
   - Read the report, verify the write fence was respected (code only in `codeFence`; every shared or unnamed file it touched named in the report, for this component's needs only), and continue orchestrating — or delegate the next scoped task.

This flow is transport-agnostic: it works identically over local stdio and the hosted data plane — the tools and `wairon-agent://` resources are the same surface.

## What goes in the brief — and what must not

A brief that re-types the working conventions is a brief that will one day omit
one, and the omission is invisible: nobody reads a prompt looking for what is not
in it. The conventions that hold for **any** delegated change live in
`sdd-implement` under **Working conventions** — never hand-edit specs, read every
write back, the lock is the human's, measure before repairing, restore a revert
proof from your own snapshot, refuse with reasoning, report what you did not do.
Point the subagent at that skill and spend the brief on what only you know:

* **The task and the fence** — `brief.ownedPaths`, `brief.readPaths`, the concrete
  change, and the branch/commit discipline if the project has one.
* **What this project does differently** — its gate commands and their current
  baselines, its tooling or line-ending quirks, the paths that are somebody else's
  work in flight. Name the project's own contributor doc rather than paraphrasing
  it: a paraphrase drifts, and the subagent cannot tell which copy is current.
* **The premise you are asking them to act on**, stated *as* a premise, so it can
  be contradicted.
* **For a Portal, what its handler shape section cannot know** — the brief's
  `## Handler shape` states the rule; add the framework this project uses and the
  handles it really hands each function, so the subagent declares exactly those in
  `injectedParams` (and removes any guessed earlier). A subagent left to guess
  writes `(req, res)` handlers, and the gate reads those as substitutions.

One design decision never goes into a brief as an instruction: **an entry**. A
subagent that meets an unreached Portal verb (`UNUSED_COMPONENT` /
`UNUSED_METHOD`) reports it; whether to model the caller, declare an entry for
real outside callers, or remove the verb is the architect's call, made in the
spec — never an entry invented in passing to make a gate green.

## Receiving the report

* **A measurement or a refusal is a delivery, not a failure.** "This lights up 362
  findings" or "the code does not do what the brief assumes, here is the proof" is
  the one thing you could not have learned without spending that context. Decide
  on it. Re-delegating "just fix it" throws the measurement away and buys the same
  question back later at full price.
* **Read the part of the report that says what was NOT done.** Skipped gates and
  untested paths are where the next wave's surprise lives, and a report that lists
  only successes has not been read until you have looked for that section.
* **Verify the write fence and the gate numbers yourself** before you build the
  next delegation on top of this one.
