#!/usr/bin/env python3
"""The install half of the optional-dependency proof: the flag must CHANGE THE OUTCOME.

`15-optional-dependency.sh` publishes two packages whose manifests differ in one character and
proves the flag survives packing, publication, storage and resolution. That is necessary and proves
nothing on its own — a flag can travel the whole path faithfully and be read by nobody.

    devbot.opt.needs   depends on devbot.opt.ghost   optional: true   -> INSTALLS
    devbot.opt.wants   depends on devbot.opt.ghost   optional: false  -> REFUSED

Neither ghost exists. THE REFUSAL IS THE HALF THAT MATTERS. An install that succeeds proves nothing
unless the version that must fail does — otherwise "it installed" is equally consistent with the
marketplace having stopped checking closures at all.

Requires a node running a marketplace built after the flag landed. Run
`update-preinstalled-package.py` for syw.system.marketplace and restart if step 0 says so.
"""

import asyncio
import json
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
REGISTRY_NAME = "local-dev"
MARKETPLACE = "syw.system.marketplace"

NEEDS = "devbot.opt.needs"
WANTS = "devbot.opt.wants"
GHOST = "devbot.opt.ghost"

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


async def installed_fqids(ws):
    result, error = await rpc(ws, "packages", "list", None)
    if error:
        bad(f"packages:list refused: {error}")
        return None
    return {row[0] for row in (result or []) if isinstance(row, (list, tuple)) and row}


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        step("0. does this node's marketplace understand the flag?")
        # THE PRECONDITION THAT DECIDES WHETHER ANYTHING BELOW MEANS ANYTHING. A marketplace built
        # before the flag refuses BOTH packages — and a run that reported that as "the required one
        # was refused, the leg works" would be reading its own staleness as a pass.
        plan, error = await rpc(ws, MARKETPLACE, "plan", [NEEDS, None, REGISTRY_NAME])
        if error:
            bad(f"plan refused: {error}")
            raise SystemExit(1)
        entries = plan if isinstance(plan, list) else (plan or {}).get("plan", [])
        ghost = next((e for e in entries if e.get("namespace") == GHOST), None)
        if ghost is None:
            note(f"{NEEDS} is not published, or its plan omits {GHOST}. Run 15 first.")
            raise SystemExit(2)
        if "optional" not in ghost:
            note("this node's marketplace does not carry `optional` on a plan entry.")
            print("        It predates the flag, so BOTH packages would be refused and the")
            print("        refusal below would prove nothing. Update syw.system.marketplace:")
            print("            python update-preinstalled-package.py syw.system.marketplace")
            print("        then restart the node and run this again.")
            raise SystemExit(2)
        ok(f"the plan carries optional={ghost['optional']!r} for {GHOST}")

        before = await installed_fqids(ws)
        if before is None:
            raise SystemExit(1)

        step("1. the OPTIONAL one installs, with its missing dependency reported")
        result, error = await rpc(
            ws, MARKETPLACE, "install", [NEEDS, None, "Interactive", None, REGISTRY_NAME]
        )
        if error:
            bad(f"{NEEDS} was refused despite declaring the dependency OPTIONAL: {error}")
        else:
            report = result or {}
            ok(f"{NEEDS} installed")
            skipped = report.get("skipped_optional") or []
            if not skipped:
                # REPORTED RATHER THAN SILENT. An install that quietly delivers fewer packages than
                # the plan named is one where nobody can tell a working optional from a broken
                # registry.
                bad(f"nothing was reported as skipped: {report}")
            elif not any(GHOST in s for s in skipped):
                bad(f"the skipped list does not name {GHOST}: {skipped}")
            else:
                ok(f"and the run reported what it skipped: {skipped[0]}")

        step("2. the REQUIRED one is refused — the half that makes step 1 mean something")
        result, error = await rpc(
            ws, MARKETPLACE, "install", [WANTS, None, "Interactive", None, REGISTRY_NAME]
        )
        if not error:
            bad(f"{WANTS} installed with a REQUIRED dependency missing: {result}")
        else:
            message = json.dumps(error, default=str)
            ok("refused")
            if GHOST not in message:
                bad(f"the refusal does not name {GHOST}: {message[:300]}")
            else:
                ok(f"naming {GHOST}, which is the thing to fix")

        step("3. what the node actually holds")
        after = await installed_fqids(ws)
        if after is None:
            raise SystemExit(1)
        if NEEDS in after:
            ok(f"{NEEDS} is installed")
        else:
            bad(f"{NEEDS} is not in the package list despite a successful install")
        if WANTS in after:
            bad(f"{WANTS} is installed even though its closure was refused")
        else:
            ok(f"{WANTS} is not installed")
        if GHOST in after:
            bad(f"{GHOST} exists on the node, so neither case proved anything")
        else:
            ok(f"{GHOST} was never installed by either run — it does not exist")

    print("\n" + ("=" * 70))
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mthe optional-dependency install leg passed\033[0m")


asyncio.run(main())
