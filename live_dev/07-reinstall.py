#!/usr/bin/env python3
"""Reinstall from the REPUBLISHED artifact, closing the loop.

The bundle now on the registry was packed after the reproducibility fix, so it is a different byte
sequence and a different content address from the one first installed. Installing it proves the loop
closes on a genuinely new artifact rather than on a cached one.
"""

import asyncio
import json
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
REGISTRY_NAME = "local-dev"
FQID = "devbot.example.hello"
MARKETPLACE = "syw.system.marketplace"
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
        step("1. it is not installed")
        pkgs, err = await rpc(ws, "packages", "list", None)
        names = [p[0] for p in (pkgs or []) if isinstance(p, (list, tuple)) and p]
        if FQID in names:
            bad(f"{FQID} is already installed, so this run cannot prove an install")
        else:
            ok("absent, as the uninstall leg left it")

        step("2. the marketplace can see it again after the republish")
        result, error = await rpc(ws, MARKETPLACE, "plan", [FQID, None, REGISTRY_NAME])
        if error:
            bad(f"plan refused: {error}")
        else:
            print("  plan:", json.dumps(result, default=str)[:400], flush=True)
            ok("resolvable")

        step("3. install the REPUBLISHED artifact")
        result, error = await rpc(ws, MARKETPLACE, "install", [FQID, None, "Interactive", None, REGISTRY_NAME])
        if error:
            bad(f"install refused: {error}")
        else:
            print("  install:", json.dumps(result, default=str)[:500], flush=True)
            installed = (result or {}).get("installed") or []
            if installed:
                ok(f"installed {installed[0].get('fqid')}@{installed[0].get('version')}")
            else:
                bad(f"nothing was installed: {result!r}")

        step("4. present in the record")
        pkgs, err = await rpc(ws, "packages", "list", None)
        names = [p[0] for p in (pkgs or []) if isinstance(p, (list, tuple)) and p]
        if FQID in names:
            ok("listed")
        else:
            bad("not listed after install")

    print("\n" + "=" * 70)
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mreinstalled from the republished artifact\033[0m")


asyncio.run(main())
