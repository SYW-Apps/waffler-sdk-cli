#!/usr/bin/env python3
"""Does a package whose required dependency is GONE still start at boot?

This is the observable that separates core's dependency-gate fix from the absence of it, and it is
the one `13-closure-serves.py` cannot see. Two reasons it cannot, both worth stating:

  * `enabled` stays TRUE either way. It is the OPERATOR'S INTENT — "I want this running" — and core
    deliberately does not clear it, so the package starts again by itself once the dependency comes
    back. Reading it as a health signal reads a flag adjacent to the question.
  * an already-RUNNING actor does not consult its dependencies per call. It finishes its life like a
    process holding a handle to a deleted file, so nothing changes in one process's window.

So the question is whether a RESTART now refuses to start it, and no single process can assert
across a restart — the same reason `04-verify-serves.py` exists apart from `03`.

## RUN IT TWICE, WITH A NODE RESTART IN BETWEEN

It reads its phase from the NODE rather than from a flag, so the two runs cannot be done in the
wrong order or the wrong number of times:

    python 14-gate-after-restart.py     # phase 1: proves all three serve, removes the leaf
    docker restart waffler-beta         # ... and wait for healthy
    python 14-gate-after-restart.py     # phase 2: the dependents must NOT have started

## THIS ONE IS ALLOWED TO GO RED, AND 13 IS NOT

`13-closure-serves.py` reports the same finding without failing, because a leg left permanently red
over somebody else's open question buries the next real failure among assertions about something
else. This leg is the opposite case: it tests exactly ONE thing, and it is red precisely because
that thing is not fixed yet. That is an ordinary failing test for a known defect, and it turns green
on the commit that closes it — which is the whole point of separating it out.

## WHY NOT `04-verify-serves.py`, WHICH IS WHAT I FIRST WROTE DOWN

Because 04 asserts a package DOES answer. Pointing it at a dependent whose dependency was removed
makes it go red for exactly the outcome we want, which is a check that reports a fix as a failure.
A test whose pass condition is inverted relative to the thing it is called on is worse than no test:
somebody reads the red and re-breaks the fix.
"""

import asyncio
import json
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
REGISTRY_NAME = "local-dev"
MARKETPLACE = "syw.system.marketplace"

APP = "devbot.dia.app"
LIB = "devbot.dia.lib"
UTIL = "devbot.dia.util"
CLOSURE = [UTIL, LIB, APP]
DEPENDENTS_OF_UTIL = [LIB, APP]

# See 12-install-closure.py for why this is a set of KNOWN arities rather than one or a minimum.
PACKAGE_ARITIES = {16, 17}
FQID_AT, VERSION_AT, ENABLED_AT = 0, 1, 10

# The two spellings of "installed but not running".
NOT_RUNNING = ("NotFound", "ServiceUnavailable")

_next_id = [0]
failures = []


def step(t):
    print(f"\n\033[36m==== {t} ====\033[0m", flush=True)


def ok(m):
    print(f"\033[32m  OK  {m}\033[0m", flush=True)


def bad(m):
    failures.append(m)
    print(f"\033[31m  FAIL  {m}\033[0m", flush=True)


def note(m):
    print(f"\033[33m  NOTE  {m}\033[0m", flush=True)


async def rpc(ws, service, command, payload):
    _next_id[0] += 1
    await ws.send(
        msgpack.packb(
            {
                "jsonrpc": "2.0",
                "id": str(_next_id[0]),
                "method": "send_message",
                "params": {"target": {"service": service, "command": command}, "payload": payload},
            },
            use_bin_type=True,
        )
    )
    raw = await asyncio.wait_for(ws.recv(), timeout=180)
    reply = msgpack.unpackb(raw, raw=False, strict_map_key=False)
    return reply.get("result"), reply.get("error")


async def held(ws):
    """{fqid: (version, enabled)}."""
    result, error = await rpc(ws, "packages", "list", None)
    if error:
        bad(f"packages:list refused: {error}")
        return None
    packages = {}
    for row in result or []:
        if not isinstance(row, (list, tuple)) or len(row) not in PACKAGE_ARITIES:
            bad(f"an installed package arrived with an unexpected arity: {row!r}")
            return None
        packages[row[FQID_AT]] = (row[VERSION_AT], row[ENABLED_AT])
    return packages


async def answers(ws, fqid):
    """(does it answer, the refusal code if not)."""
    _, error = await rpc(ws, fqid, "echo", "gate probe")
    if error is None:
        return True, None
    return False, (error.get("data") or {}).get("code", "?")


async def phase_one(ws, state):
    step("PHASE 1 — prove all three serve, then remove the leaf")
    for fqid in CLOSURE:
        serving, code = await answers(ws, fqid)
        if not serving:
            # REFUSES TO START rather than reporting a failure that describes something else.
            note(f"{fqid} is not serving ({code}).")
            print("        This phase needs the whole closure INSTALLED, GRANTED and RUNNING.")
            print("        Run 11 -> 12 -> restart -> 13, then start here.")
            sys.exit(2)
        ok(f"{fqid} serves")

    _, error = await rpc(ws, "packages", "uninstall", [UTIL])
    if error:
        # THE OTHER SAFE DESIGN. If uninstall grows a refusal while required dependents exist, this
        # phase can no longer set up its own question — and that is a pass, not a failure.
        ok("the uninstall was REFUSED while required dependents are installed")
        print(f"  {json.dumps(error)[:400]}")
        print("\n  Nothing further to check: the node cannot be put into the state phase 2 asks")
        print("  about, which is the stronger of the two safe answers.")
        return
    ok(f"{UTIL} removed")

    serving_now = [f for f in DEPENDENTS_OF_UTIL if (await answers(ws, f))[0]]
    if serving_now:
        # EXPECTED AT THIS POINT, under every design. A running actor does not re-check its
        # dependencies, so this says nothing about the gate — recorded so phase 2's result is not
        # read as a change from here.
        note(f"still serving, as a running actor does either way: {', '.join(serving_now)}")

    print("\n\033[36m  PHASE 1 COMPLETE.\033[0m Now restart the node and run this again:")
    print("      docker restart waffler-beta   # wait for healthy")
    print("      python 14-gate-after-restart.py")


async def phase_two(ws, state):
    step("PHASE 2 — after the restart, did the dependents start?")
    # THE ONLY QUESTION HERE. `enabled` is deliberately NOT asserted: core keeps it set so the
    # package starts again once the dependency returns, so it is true under both outcomes and
    # reading it would answer a question nobody asked.
    for fqid in DEPENDENTS_OF_UTIL:
        if fqid not in state:
            ok(f"{fqid} was removed along with its dependency")
            continue
        serving, code = await answers(ws, fqid)
        print(f"  {fqid}: enabled={state[fqid][1]!r}  answers={serving}  code={code}")
        if serving:
            bad(
                f"{fqid} started at boot with its required {UTIL} uninstalled — "
                f"the gate is still on the enable path only"
            )
        elif code in NOT_RUNNING:
            ok(f"{fqid} did not start")
        else:
            # A refusal for some OTHER reason is not evidence for either answer, and calling it one
            # would be reading a green from the wrong artifact.
            bad(f"{fqid} refused with {code}, which is neither running nor the boot gate")

    # `NotFound` IS NECESSARY AND NOT SUFFICIENT. It means "no handler registered", which is equally
    # what a package that never started for some unrelated reason returns — the bus cannot say WHY
    # something is absent. The gate's own diagnostic is a boot-time log line and a `package.error`
    # event, and the event has already fired by the time this script connects, so neither is
    # reachable from here. Naming the discriminator rather than implying this run established it:
    print("\n  The bus cannot say WHY a handler is absent. Confirm the CAUSE in the boot log:")
    print("      docker logs waffler-beta 2>&1 | grep DependencyUnmet")
    print("  Expect one line per dependent, naming it and the missing dependency and range.")

    step("RESTORE — put the closure back")
    _, error = await rpc(
        ws, MARKETPLACE, "install", [APP, None, "Interactive", None, REGISTRY_NAME]
    )
    if error:
        bad(f"restoring the closure refused: {error}")
    else:
        ok("closure reinstalled — it needs one more restart before it serves again")


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        state = await held(ws)
        if state is None:
            sys.exit(1)
        missing = [f for f in DEPENDENTS_OF_UTIL if f not in state]
        if missing:
            note(f"not installed: {missing}. Run 11 -> 12 -> restart -> 13 first.")
            sys.exit(2)

        # THE PHASE COMES FROM THE NODE, not from a flag or an argument. A flag can be passed twice,
        # or in the wrong order, and the run would then assert the wrong thing while looking
        # deliberate. Whether the leaf is installed is the fact that actually distinguishes them.
        if UTIL in state:
            await phase_one(ws, state)
        else:
            await phase_two(ws, state)

    print("\n" + ("=" * 70))
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mok\033[0m")


asyncio.run(main())
