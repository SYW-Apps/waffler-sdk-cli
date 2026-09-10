#!/usr/bin/env python3
"""After a node restart: does the package installed from the registry actually serve?

## WHY THIS IS A SEPARATE SCRIPT

The previous run ended on `NotFound: no handler registered`. That is not the permission gate — the
grant moved the error from `AccessDenied` to `NotFound`, which is exactly what a working grant looks
like — it is the node's INSTALL/RUN split: a package's actor is registered at boot, so one installed
afterwards is present in the store and absent from the router until the node restarts.

So the honest test of "it works" has to span a restart, and a script that ran before and after in one
process would be asserting across a discontinuity it cannot see.
"""

import asyncio
import json
import os
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
# Named by the caller so the closure legs can point this at each member of a graph in turn. The
# default is the single package the earlier legs publish, so running this bare still does what it
# always did. VERSION is no longer pinned: 11-dependency-closure.sh publishes a fresh patch every
# run, and a hardcoded version here would fail for a reason that is not "does it serve".
FQID = os.environ.get("CYCLE_FQID", "devbot.example.hello")
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
        step("1. the package survived the restart")
        recorded_version = None
        pkgs, err = await rpc(ws, "packages", "list", None)
        if err:
            bad(f"packages:list refused: {err}")
        else:
            rows = {p[0]: p for p in (pkgs or []) if isinstance(p, (list, tuple)) and p}
            if FQID in rows:
                row = rows[FQID]
                recorded_version = row[1]
                ok(f"{FQID} {row[1]} {row[3]} identity={row[2]}")
            else:
                bad(f"{FQID} is not installed after the restart (have: {sorted(rows)})")

        step("2. the operator's grant survived the restart")
        b, err = await rpc(ws, "security", "bindings.list", None)
        if err:
            bad(f"bindings.list refused: {err}")
        else:
            text = json.dumps(b, default=str)
            if "call_devbot_example_hello" in text:
                ok("the binding is still there")
            else:
                bad("the operator's binding did not survive the restart")

        step("3. DOES IT ANSWER? — the observation that separates installed from working")
        result, error = await rpc(ws, FQID, "echo", "hello after restart")
        if error:
            bad(f"it did not answer: {json.dumps(error)[:400]}")
        else:
            print("  reply:", json.dumps(result, indent=2, default=str)[:600], flush=True)
            if not isinstance(result, dict):
                bad(f"unexpected reply shape: {result!r}")
            else:
                if result.get("package") == FQID:
                    ok("it identified itself — so THIS artifact answered, not something echoing")
                else:
                    bad(f"the reply names {result.get('package')!r}")
                if result.get("echo") == "hello after restart":
                    ok("and returned the payload it was handed")
                else:
                    bad(f"the echo came back as {result.get('echo')!r}")
                # AGAINST WHAT THE NODE RECORDED, not against a constant. The two can disagree, and
                # that disagreement is the finding: the row says one version and the artifact
                # answering reports another, which is a stale module still loaded under a record
                # that has moved on. Asserted with an `else`, because the version check here was
                # previously an `if` with no else — it said nothing at all when it disagreed, which
                # is the one case worth hearing about.
                answered = result.get("version")
                if recorded_version is None:
                    bad("no recorded version to compare against — step 1 did not find the package")
                elif answered == recorded_version:
                    ok(f"and the version it reports is the one the node recorded: {answered}")
                else:
                    bad(
                        f"the node records {recorded_version} but the artifact answering "
                        f"reports {answered} — a stale module is loaded"
                    )

        step("4. a capability it does NOT serve is named rather than answered")
        # A package that answered everything would report success for a caller asking for something it
        # does not serve — the failure a fixture must never model, and the check that proves the reply
        # above came from the dispatch rather than from a catch-all.
        result, error = await rpc(ws, FQID, "not_a_capability", None)
        if error:
            ok(f"refused: {json.dumps(error)[:120]}")
        elif isinstance(result, dict) and result.get("error") == "unknown capability":
            ok("named as unknown, and not echoed")
        else:
            bad(f"an unserved capability was answered: {result!r}")

    print("\n" + "=" * 70)
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mthe package installed from the registry SERVES\033[0m")


asyncio.run(main())
