#!/usr/bin/env python3
"""CONSENT FOR A WHOLE CLOSURE, on the live node: the leak refused, and the answer that is enough.

Run `18-consent-closure.sh` first. It publishes

       devbot.cst.app ──▶ devbot.cst.lib

where BOTH declare a REQUIRED permission group with the same id, `net`.

Core applies an install's decisions by BARE id. The marketplace used to hand ONE decisions set to every
package of a closure, so approving the root's `net` also approved the dependency's `net` — a request the
operator was never shown. Decisions are now keyed `namespace@version`, one bundle's set each, and an
Interactive run refuses the WHOLE closure while any required request of any package lacks an approving
answer.

WHAT THIS PROVES, in the order that makes each step mean something:
  1. the preview shows BOTH packages' `net`, as two requests of two packages;
  2. THE LEAK: answering only the root's `net` is REFUSED, naming the dependency's, and NOTHING lands;
  3. the flat, unkeyed shape is refused rather than reinterpreted, and nothing lands;
  4. an answer per package installs both, and each record approves its OWN `net`;
  5. each package then ENABLES on its own approval. Installing is not enabling, and the enable gate is
     where an unapproved required group refuses, so this is where the answers are shown to MATTER.
The refusals come first: an install that succeeds proves nothing unless the ones that must fail do.
"""

import asyncio
import json
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
REGISTRY_NAME = "local-dev"
MARKETPLACE = "syw.system.marketplace"

APP = "devbot.cst.app"
LIB = "devbot.cst.lib"
GROUP = "net"

# `packages:list` is POSITIONAL; see 12-install-closure.py for why this is a set of known arities.
PACKAGE_ARITIES = {16, 17}
FQID_AT, VERSION_AT, ENABLED_AT, APPROVED_GROUPS_AT = 0, 1, 10, 13

_next_id = [0]
failures = []


def step(t):
    print(f"\n\033[36m==== {t} ====\033[0m", flush=True)


def ok(m):
    print(f"\033[32m  OK  {m}\033[0m", flush=True)


def bad(m):
    failures.append(m)
    print(f"\033[31m  FAIL  {m}\033[0m", flush=True)


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
    raw = await asyncio.wait_for(ws.recv(), timeout=300)
    reply = msgpack.unpackb(raw, raw=False, strict_map_key=False)
    return reply.get("result"), reply.get("error")


async def held(ws):
    """{fqid: (version, enabled, approved_group_ids)} — what the node records, not what a report says."""
    result, error = await rpc(ws, "packages", "list", None)
    if error:
        bad(f"packages:list refused: {error}")
        return None
    packages = {}
    for row in result or []:
        if not isinstance(row, (list, tuple)) or len(row) not in PACKAGE_ARITIES:
            bad(f"an installed package arrived with an unexpected arity: {row!r}")
            return None
        packages[row[FQID_AT]] = (row[VERSION_AT], row[ENABLED_AT], row[APPROVED_GROUPS_AT])
    return packages


async def nothing_landed(ws, versions, when):
    """A refusal that installed part of the closure first is a refusal in name only."""
    state = await held(ws)
    if state is None:
        return
    landed = [f"{f}@{v}" for f, v in versions.items() if state.get(f, (None,))[0] == v]
    if landed:
        bad(f"{when}: these landed anyway — {landed}")
    else:
        ok(f"{when}: nothing from the closure landed")


def expect_refusal(error, result, code, when):
    """The refusal, and the code it was refused WITH. Returns the error text for further checks."""
    if not error:
        bad(f"{when}: it INSTALLED — {json.dumps(result, default=str)[:400]}")
        return ""
    message = json.dumps(error, default=str)
    if code in message:
        ok(f"{when}: refused as {code}")
    else:
        bad(f"{when}: refused, but not as {code}: {message[:400]}")
    return message


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        step("0. the node must already hold the local registry")
        result, error = await rpc(ws, MARKETPLACE, "registries.list", None)
        if error:
            bad(f"registries.list refused: {error}")
            raise SystemExit(1)
        if REGISTRY_NAME not in [r.get("name") for r in (result or [])]:
            print(f"\033[31m  {REGISTRY_NAME} is not registered. Run 02-install.py first.\033[0m")
            raise SystemExit(2)
        ok(f"{REGISTRY_NAME} is registered")

        # THIS LEG OWNS ITS STARTING STATE, and it has to. Step 5's enable is a NO-OP on a package
        # whose actor is still MOUNTED from a previous run: core's enable returns Ok early when the
        # router already resolves the fqid ("the router is the fact"), and that early return comes
        # BEFORE the set_enabled(true) that persists the flag. So a second run installs a new version
        # over a mounted one, the new record reads enabled=false, the enable reports success, and the
        # flag never flips. That is a real defect in the UPDATE path and it is written down as one —
        # but left here it would read as this leg failing, which is how it wasted an hour once.
        # The dependent first: uninstalling a dependency out from under its dependent is refused.
        for fqid in (APP, LIB):
            _result, error = await rpc(ws, "packages", "uninstall", {"fqid": fqid})
            if error and "NotFound" not in json.dumps(error, default=str):
                bad(f"could not clear a previous run's {fqid}: {json.dumps(error, default=str)[:300]}")
        state = await held(ws)
        if state is not None and (APP in state or LIB in state):
            bad(f"a previous run's copies are still installed: {[f for f in (APP, LIB) if f in state]}")
        else:
            ok("the node holds neither package: this run starts from nothing")

        step("1. THE PREVIEW shows both packages' `net`, as two requests of two packages")
        preview, error = await rpc(ws, MARKETPLACE, "preview_closure", [APP, None, REGISTRY_NAME])
        if error:
            bad(f"preview_closure refused: {error}")
            raise SystemExit(1)
        previewed = {p.get("namespace"): p for p in (preview or {}).get("previewed", [])}
        if APP not in previewed or LIB not in previewed:
            # REFUSES TO START rather than reporting failures: without both previewed, the node already
            # holds a version, nothing is left to refuse, and every step below would pass vacuously.
            print(
                f"\033[31m  the preview does not cover both packages (previewed={sorted(previewed)}, "
                f"already_present={(preview or {}).get('already_present')}). Run 18 to publish fresh versions.\033[0m"
            )
            raise SystemExit(2)
        versions = {fqid: previewed[fqid]["version"] for fqid in (APP, LIB)}
        for fqid in (APP, LIB):
            groups = ((previewed[fqid].get("requests") or {}).get("permission_groups")) or []
            net = [g for g in groups if g.get("id") == GROUP]
            if net and net[0].get("required") is True:
                ok(f"{fqid}@{versions[fqid]} asks for '{GROUP}' (required), sensitivity {net[0].get('sensitivity')!r}")
            else:
                bad(f"{fqid}@{versions[fqid]} is not previewed as requiring '{GROUP}': {groups}")

        def key(fqid):
            return f"{fqid}@{versions[fqid]}"

        step("2. THE LEAK: an answer about the root's `net` must not reach the dependency's")
        result, error = await rpc(
            ws, MARKETPLACE, "install", [APP, None, "Interactive", {key(APP): {"groups": {GROUP: True}}}, REGISTRY_NAME]
        )
        message = expect_refusal(error, result, "ConsentIncomplete", "answering only the root")
        if message:
            gap = f"{key(LIB)}: permission group '{GROUP}' (required)"
            if gap in message:
                ok(f"and it names {key(LIB)}'s '{GROUP}' — the request nobody answered")
            else:
                bad(f"the refusal does not name {gap!r}: {message[:500]}")
            if f"{key(APP)}: permission group" in message:
                bad(f"it also names {key(APP)}'s group, which WAS answered: {message[:500]}")
        await nothing_landed(ws, versions, "after the refused leak")

        step("3. the FLAT shape is refused, never reinterpreted as the root's")
        result, error = await rpc(
            ws, MARKETPLACE, "install", [APP, None, "Interactive", {"groups": {GROUP: True}}, REGISTRY_NAME]
        )
        expect_refusal(error, result, "InvalidDecisions", "an unkeyed decisions set")
        await nothing_landed(ws, versions, "after the refused flat shape")

        step("4. AN ANSWER PER PACKAGE installs both, each approving its OWN group")
        decisions = {key(APP): {"groups": {GROUP: True}}, key(LIB): {"groups": {GROUP: True}}}
        result, error = await rpc(ws, MARKETPLACE, "install", [APP, None, "Interactive", decisions, REGISTRY_NAME])
        if error:
            bad(f"a complete, keyed answer was refused: {json.dumps(error, default=str)[:500]}")
        else:
            report = result or {}
            installed = {r.get("fqid"): r for r in report.get("installed") or []}
            for fqid in (LIB, APP):
                if fqid in installed:
                    ok(f"the report installed {fqid}@{installed[fqid].get('version')}")
                else:
                    bad(f"the report does not install {fqid}: {json.dumps(report, default=str)[:500]}")
            if report.get("unapplied_decisions"):
                bad(f"answers were left unapplied: {report['unapplied_decisions']}")
            else:
                ok("every answer reached the package it names")
            if report.get("awaiting_review"):
                bad(f"something still awaits review after a complete answer: {report['awaiting_review']}")
            else:
                ok("nothing awaits review")

        state = await held(ws)
        if state is not None:
            for fqid in (LIB, APP):
                if fqid not in state:
                    bad(f"{fqid} is not in the node's package list")
                    continue
                version, _enabled, approved = state[fqid]
                if version != versions[fqid]:
                    bad(f"{fqid} is at {version}, not the {versions[fqid]} this run answered for")
                elif GROUP not in (approved or []):
                    bad(f"{fqid}@{version} records no approval of its own '{GROUP}': {approved}")
                else:
                    ok(f"{fqid}@{version} is installed and records its OWN '{GROUP}' approved")

        step("5. ENABLE — the gate each package's own approval exists for")
        # INSTALLING IS NOT ENABLING on this core: `install_zip` writes `enabled: false`, and enabling is
        # its own verb. The enable gate is where a REQUIRED group that is not approved refuses
        # (`PermissionUnapproved`), so this is the step that proves the answers did more than land in a
        # record — each package passes its OWN gate on its OWN approval. (This leg first asserted
        # `enabled` straight after the install and failed there, correctly: that was the leg being wrong.)
        # The dependency first, because the root's enable refuses while a required dependency is not
        # running (`DependencyUnmet`).
        for fqid in (LIB, APP):
            result, error = await rpc(ws, "packages", "enable", {"fqid": fqid})
            if error:
                bad(f"enabling {fqid} was refused: {json.dumps(error, default=str)[:400]}")
            else:
                ok(f"{fqid}: enable accepted")
        state = await held(ws)
        if state is not None:
            for fqid in (LIB, APP):
                row = state.get(fqid)
                if row and row[0] == versions[fqid] and row[1] is True:
                    ok(f"{fqid}@{row[0]} is enabled on its own approval")
                else:
                    bad(f"{fqid} is not enabled at {versions[fqid]} after the enable: {row!r}")

    print("\n" + "=" * 70)
    if failures:
        print(f"\033[31m{len(failures)} failure(s)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mthe consent leg passed\033[0m")


asyncio.run(main())
