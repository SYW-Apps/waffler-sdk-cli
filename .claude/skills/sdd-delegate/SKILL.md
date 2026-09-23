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

**Why live briefs**: a brief is composed from the CURRENT spec tree on every call. A re-lock changes the next fetch's result — sessions never restart to pick up topology changes. Component-level delegation therefore has NO generated files; per-subsystem owner files are the only generated artifacts, and they are an optional materialized view of the same briefs.

**Project guidance**: a project may carry user-owned per-agent guidance in `.wai/agents/<agent-id>.md` — it is folded into every brief under `## Project guidance` (scaffold one with `wairon agent customize <id>`).

## Workflow Rules
1. **Discover the target agent**:
   - Call the `listAgents` MCP tool, or list MCP resources and look for `wairon-agent://<agentId>` entries.
   - Pick the agent whose `ownedPaths`/domain matches the task. If no agent fits, stop and tell the user the topology has a gap.
2. **Fetch the LIVE brief**:
   - Call `sdd_get_agent_brief(agentId)` (or read the `wairon-agent://<agentId>` resource).
   - The brief carries: `agentId`, `name`, `template`, `domainRoot?`, `ownedPaths`, `readPaths?`, `instructions`, `variantGuidance?`, and — when the project opted into `execution.tier` — `profile` and `budget`.
   - **Never reuse a brief across delegations or after a re-lock** — fetch fresh per delegation; the call is cheap and the brief is always current.
3. **Spawn a GENERIC subagent from the brief**:
   - Prompt: `brief.instructions`, plus the concrete task description.
   - Write fence: the subagent may only modify files matching `brief.ownedPaths` (within `brief.domainRoot` when set).
   - Required first reading: `brief.readPaths` — the subagent reads these before any edit.
   - Pass `brief.variantGuidance` along when present.
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
   - Read the report, verify the write fence was respected, and continue orchestrating — or delegate the next scoped task.

This flow is transport-agnostic: it works identically over local stdio and the hosted data plane — the tools and `wairon-agent://` resources are the same surface.
