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
                # AGAINST WHAT THE NODE RECORDED, not against a constant. Asserted with an `else`,
                # because this was previously an `if` with no else and said nothing at all when the
                # two disagreed, which is the one case worth hearing about.
                #
                # ## THIS COMPARISON IS ONLY VALID FOR A SCAFFOLDED PACKAGE, and that is a premise
                # ## of the leg rather than a property of the two numbers.
                #
                # They are DIFFERENT NUMBERS that are ALLOWED TO DIFFER. The reply's `version` is
                # the artifact's self-report, which derives from `CARGO_PKG_VERSION` — the CRATE.
                # The node's recorded version is the BUNDLE version, which the packer stamps from
                # `waffler.json`. Nothing requires those to agree, and across the hand-maintained
                # sim packages they deliberately do not: `syw.network.http`'s crate is 1.0.0 against
                # a 1.1.2 bundle.
                #
                # It holds HERE because `FQID` is a package `waffler scaffold` created, and scaffold
                # writes `Cargo.toml` and `waffler.json` from one `--version` input — so equality is
                # the intent for this subject, and a divergence really is the stale-module finding.
                # Point this leg at a sim package and the same check reports a stale module on a
                # perfectly healthy node. That is not hypothetical: a version const frozen at
                # scaffold time produced exactly that false alarm here once, and it was believed
                # long enough to be written into a plan. Deriving the const fixed the lying
                # self-report; it did NOT make these two numbers equivalent, so the premise has to
                # be stated rather than inherited.
                #
                # The sound signal, if this ever needs to work for a package whose versions are
                # decoupled, is the installed REVISION rather than a version string — the node
                # rewrites `rev/CURRENT` to a content-named revision on replacement. Not built here:
                # it needs a field this reply does not carry, and the scoped check is correct for
                # the only subject this leg has.
                answered = result.get("version")
                if recorded_version is None:
                    bad("no recorded version to compare against — step 1 did not find the package")
                elif answered == recorded_version:
                    ok(f"and the version it reports is the one the node recorded: {answered}")
                else:
                    bad(
                        f"the node records {recorded_version} but the artifact answering "
                        f"reports {answered} — for a SCAFFOLDED package these are set from one "
                        f"input, so the OLD module is still loaded (D15: a package library is never "
                        f"unloaded and is replaced at the same path, so the node serves the old "
                        f"image until it restarts). If {FQID} is no longer a scaffolded package, "
                        f"check that before blaming the node: a crate version and a bundle version "
                        f"are allowed to differ, and this comparison is not valid for one that does"
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
