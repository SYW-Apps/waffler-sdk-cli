#!/usr/bin/env python3
"""Uninstall, and prove it STOPPED the package serving.

## WHY THIS IS RUN NOW AND NOT EARLIER

An earlier run asserted "it must stop answering" and passed — while the package had never answered in
that process at all, because it was installed and not yet running. The assertion was true and proved
nothing: the observable was right and the reason was not.

The only version of that assertion worth making is one where the call SUCCEEDS first. That is only
possible after the restart, which is why this leg runs here.
"""

import asyncio
import json
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
FQID = "devbot.example.hello"
_next_id = [0]
failures = []


def step(t):
    print(f"\n\033[36m==== {t} ====\033[0m", flush=True)


def ok(m):
    print(f"\033[32m  OK    {m}\033[0m", flush=True)


def bad(m):
    failures.append(m)
    print(f"\033[31m  FAIL  {m}\033[0m", flush=True)


def frame(method, params):
    _next_id[0] += 1
    return msgpack.packb({"jsonrpc": "2.0", "id": str(_next_id[0]), "method": method, "params": params}, use_bin_type=True)


async def rpc(ws, service, command, payload):
    await ws.send(frame("send_message", {"target": {"service": service, "command": command}, "payload": payload}))
    raw = await asyncio.wait_for(ws.recv(), timeout=180)
    reply = msgpack.unpackb(raw, raw=False, strict_map_key=False)
    return reply.get("result"), reply.get("error")


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        step("1. BASELINE — it answers right now")
        result, error = await rpc(ws, FQID, "echo", "still here")
        if error or not isinstance(result, dict) or result.get("echo") != "still here":
            bad(f"the baseline call did not succeed, so nothing below can prove anything: {error or result}")
            print("\033[31mstopping: without a working baseline the uninstall assertion is vacuous\033[0m")
            sys.exit(1)
        ok("it answers")

        step("2. UNINSTALL")
        result, error = await rpc(ws, "packages", "uninstall", [FQID])
        if error:
            bad(f"uninstall refused: {error}")
        else:
            ok("uninstall reported success")

        step("3. it must STOP answering — the transition, not the state")
        result, error = await rpc(ws, FQID, "echo", "after uninstall")
        if error:
            ok(f"refused: {json.dumps(error)[:140]}")
        else:
            # AN UNINSTALLED PACKAGE THAT STILL ANSWERS is the worst available outcome: the record is
            # gone and the code is still executing, so nothing in the node can report what is running.
            bad(f"it ANSWERED after being uninstalled: {result!r}")

        step("4. and it must be gone from the record")
        pkgs, err = await rpc(ws, "packages", "list", None)
        if err:
            bad(f"packages:list refused: {err}")
        else:
            names = [p[0] for p in (pkgs or []) if isinstance(p, (list, tuple)) and p]
            if FQID in names:
                bad(f"{FQID} is still listed")
            else:
                ok(f"delisted; {len(names)} packages remain")

    print("\n" + "=" * 70)
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32muninstall stops the package serving AND removes the record\033[0m")


asyncio.run(main())
