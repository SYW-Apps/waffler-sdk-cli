#!/usr/bin/env python3
"""Install a whole DEPENDENCY CLOSURE through the marketplace, on the live node.

Every install this harness has done was one package declaring nothing. The marketplace's closure
lane — resolve, refuse-if-unresolvable, install in the order given, skip what is present — is
unit-tested and had never been handed a graph by a real registry.

Run `11-dependency-closure.sh` first: it publishes the diamond this installs.

       app ────────┐
        │          │
        ▼          ▼
       lib ─────▶ util

WHAT A CLOSURE INSTALL CAN GET WRONG THAT ONE PACKAGE CANNOT
  - install the root and none of its dependencies, and report success;
  - install them in an order where something arrives before what it needs;
  - install the half of a closure that resolved, leaving the node to fail later, elsewhere;
  - re-install a dependency already present, at a different version, silently.

Each of those is asserted against below, and the third is asserted by DOING it: a package whose
dependency does not exist is installed and its install must be refused with nothing landing.
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
ORPHAN = "devbot.dia.orphan"
# The root of the closure where two packages ask for incompatible ranges of one dependency.
CONFLICT_ROOT = "devbot.cnf.app"
GHOST = "devbot.dia.ghost"

# The authored graph, restated here for ONE job: checking order. It is small enough to read and the
# script that publishes it is the other half of this pair.
DECLARES = {APP: [LIB, UTIL], LIB: [UTIL], UTIL: []}

_next_id = [0]
failures = []


def step(title):
    print(f"\n\033[36m==== {title} ====\033[0m", flush=True)


def ok(msg):
    print(f"\033[32m  OK  {msg}\033[0m", flush=True)


def bad(msg):
    failures.append(msg)
    print(f"\033[31m  FAIL  {msg}\033[0m", flush=True)


def frame(method, params):
    _next_id[0] += 1
    return msgpack.packb(
        {"jsonrpc": "2.0", "id": str(_next_id[0]), "method": method, "params": params},
        use_bin_type=True,
    )


async def rpc(ws, service, command, payload):
    """One bus command through the bridge — the seam the browser uses. Returns (result, error)."""
    params = {"target": {"service": service, "command": command}, "payload": payload}
    await ws.send(frame("send_message", params))
    raw = await asyncio.wait_for(ws.recv(), timeout=180)
    reply = msgpack.unpackb(raw, raw=False, strict_map_key=False)
    return reply.get("result"), reply.get("error")


def show(label, value, limit=1600):
    text = json.dumps(value, indent=2, default=str)
    if len(text) > limit:
        text = text[:limit] + "\n  ..."
    print(f"  {label}: {text}", flush=True)


def namespaces_of(plan):
    return [e.get("namespace") for e in (plan or [])]


def fqid_of(entry):
    """An install-report entry, whichever of its two shapes it arrived in."""
    if isinstance(entry, dict):
        return entry.get("fqid")
    # `already_present` is `fqid@version`, and an fqid contains dots but never an `@`.
    return str(entry).split("@", 1)[0]


def check_order(order, where):
    """Every package must appear after everything it declares."""
    missing = [n for n in DECLARES if n not in order]
    if missing:
        bad(f"{where}: the closure omits {missing} — got {order}")
        return
    for package, declared in DECLARES.items():
        for dependency in declared:
            if order.index(dependency) > order.index(package):
                # NAMED, because which pair inverted is the finding. "not topological" sends a
                # reader back to re-derive a graph they are already holding.
                bad(f"{where}: {package} comes before {dependency}, which it declares — {order}")
                return
    ok(f"{where}: every package appears after everything it declares — {' -> '.join(order)}")


# `packages:list` replies with POSITIONAL arrays — core's `InstalledPackage`, not a named map. The
# marketplace's replies are named maps; core's are not, and assuming one shape for both is how this
# leg first reported all three packages missing while they were sitting in the reply.
#
# Read by index, which is a wire a field inserted upstream silently shifts. Two things make that
# survivable rather than a trap: only index 0 and 10 are read, index 0 cannot move (an append goes
# to the end), and the ARITY IS ASSERTED — so a field added upstream fails this loudly here instead
# of quietly turning some other package's `enabled` into the answer.
PACKAGE_FIELDS = 16
FQID_AT = 0
VERSION_AT = 1
ENABLED_AT = 10


async def installed_set(ws):
    """{fqid: (version, enabled)} for everything the node holds."""
    result, error = await rpc(ws, "packages", "list", None)
    if error:
        bad(f"packages:list refused: {error}")
        return None
    rows = result if isinstance(result, list) else (result or {}).get("packages", [])
    packages = {}
    for row in rows:
        if isinstance(row, dict):
            packages[row.get("fqid")] = (row.get("version"), row.get("enabled"))
            continue
        if not isinstance(row, list) or len(row) != PACKAGE_FIELDS:
            bad(
                f"an installed package arrived with {len(row) if hasattr(row, '__len__') else '?'} "
                f"fields, not {PACKAGE_FIELDS} — this leg reads it by index and must not guess"
            )
            return None
        packages[row[FQID_AT]] = (row[VERSION_AT], row[ENABLED_AT])
    return packages


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        step("0. the node must already hold the local registry")
        # A PRECONDITION, NOT A STEP. Adding it here would let this script pass on a node where
        # nothing else in the harness had ever run, which is not the environment it claims to test.
        result, error = await rpc(ws, MARKETPLACE, "registries.list", None)
        if error:
            bad(f"registries.list refused: {error}")
            raise SystemExit(1)
        if REGISTRY_NAME not in [r.get("name") for r in (result or [])]:
            print(f"\033[31m  {REGISTRY_NAME} is not registered. Run 02-install.py first.\033[0m")
            raise SystemExit(2)
        ok(f"{REGISTRY_NAME} is registered")

        step("1. the closure the node resolves for the root")
        # No range: the marketplace asks for `*` and the registry picks the highest published, which
        # is what a person clicking "install" gets.
        result, error = await rpc(ws, MARKETPLACE, "plan", [APP, None, REGISTRY_NAME])
        if error:
            bad(f"plan refused: {error}")
            raise SystemExit(1)
        show("plan", result)
        plan = result if isinstance(result, list) else (result or {}).get("plan", [])
        check_order(namespaces_of(plan), "the plan the node received")

        step("2. INSTALL THE ROOT — all three must land, or none")
        before = await installed_set(ws)
        # InstallReq is positional: [namespace, range, trigger, decisions, registry].
        result, error = await rpc(
            ws, MARKETPLACE, "install", [APP, None, "Interactive", None, REGISTRY_NAME]
        )
        if error:
            bad(f"install refused: {error}")
        else:
            show("install report", result)
            report = result or {}
            # The report's own plan is what the run WORKED FROM, which is not necessarily the plan
            # step 1 saw — a second resolve could differ. Checked separately for that reason.
            check_order(namespaces_of(report.get("plan")), "the plan the install worked from")
            # THE TWO LISTS ARE NOT THE SAME SHAPE. `installed` carries records — uuid, fqid,
            # version, awaiting_review — because a fresh install has a uuid and a review list to
            # report. `already_present` carries `fqid@version` strings, because there is nothing new
            # to say about a package that did not move. Both are read, neither is assumed.
            landed = {
                fqid_of(x)
                for x in (report.get("installed") or []) + (report.get("already_present") or [])
            }
            for expected in (UTIL, LIB, APP):
                if expected in landed:
                    ok(f"the report accounts for {expected}")
                else:
                    bad(f"the report does not account for {expected}: {report}")

        step("3. what the NODE holds — the report is a claim, this is the fact")
        after = await installed_set(ws)
        if after is None:
            raise SystemExit(1)
        # WHAT THE REGISTRY OFFERED, so "the node has it" can be checked against a version rather
        # than against the mere presence of a row.
        offered = {e["namespace"]: e.get("resolved_version") for e in plan}
        for expected in (UTIL, LIB, APP):
            if expected not in after:
                bad(f"{expected} is NOT in the node's package list")
                continue
            version, enabled = after[expected]
            if enabled is not True:
                # INSTALLED AND DISABLED IS THE FAILURE THIS WHOLE LEG IS ABOUT. A package whose
                # required dependency is absent installs and is held disabled by the enable gate, so
                # a closure delivered in the wrong order lands as three rows and a root that never
                # runs — which a presence check alone would call a pass.
                bad(f"{expected} is installed but NOT enabled: {enabled!r}")
            elif version != offered.get(expected):
                # THE ROW WAS ALREADY THERE FROM AN EARLIER RUN. `11-dependency-closure.sh` publishes
                # a fresh patch every time, so on a paired run the node must MOVE to it. A leg that
                # only checked presence would report success while the node kept a stale build —
                # indistinguishable, from the outside, from an install that did nothing.
                bad(
                    f"{expected} is at {version} but the registry offered "
                    f"{offered.get(expected)} — the install did not move it"
                )
            else:
                ok(f"{expected} is installed and enabled at {version}")
        if before is not None:
            fresh = sorted(set(after) - set(before))
            moved = sorted(
                f"{k} {before[k][0]} -> {after[k][0]}"
                for k in set(after) & set(before)
                if before[k][0] != after[k][0]
            )
            print(f"  this run added: {fresh}")
            print(f"  this run moved: {moved}")

        step("4. A DEPENDENCY THAT CANNOT RESOLVE MUST STOP THE WHOLE INSTALL")
        # Installing the half of a closure that resolved produces a node whose package is missing a
        # dependency, and THAT failure surfaces later, somewhere else, as something else entirely.
        result, error = await rpc(
            ws, MARKETPLACE, "install", [ORPHAN, None, "Interactive", None, REGISTRY_NAME]
        )
        if not error:
            bad(f"the orphan installed despite an unresolvable dependency: {result}")
        else:
            message = json.dumps(error, default=str)
            ok("refused")
            print(f"  {message}")
            if GHOST in message:
                ok(f"and the refusal names {GHOST}, which is the thing to fix")
            else:
                bad(f"the refusal does not name {GHOST}, so it does not say what to fix")

        step("4b. AND SO MUST A CLOSURE NOBODY CAN SATISFY")
        # A DIFFERENT WAY TO BE UNRESOLVABLE, and the registry reports it through the same field, so
        # this is really asking whether the marketplace refuses on the FIELD or on the one cause it
        # was written against. `devbot.cnf.app` wants util ^0.1 and its lib wants util ^0.2; both
        # minors are published and no single version satisfies both.
        result, error = await rpc(
            ws, MARKETPLACE, "install", [CONFLICT_ROOT, None, "Interactive", None, REGISTRY_NAME]
        )
        if not error:
            bad(f"a closure with no satisfiable version installed anyway: {result}")
        else:
            message = json.dumps(error, default=str)
            ok("refused")
            print(f"  {message[:400]}")
            for asked in ("^0.1.0", "^0.2.0"):
                if asked not in message:
                    bad(f"the refusal does not name {asked}, so it does not say which asks disagree")
            conflict_state = await installed_set(ws)
            if conflict_state is None:
                raise SystemExit(1)
            if CONFLICT_ROOT in conflict_state:
                bad(f"{CONFLICT_ROOT} was installed even though its closure was refused")
            else:
                ok(f"and nothing from it landed")

        step("5. AND NOTHING FROM THAT CLOSURE LANDED")
        # A refusal that installed the resolvable part first is a refusal in name only, and the node
        # is then carrying a package nobody chose.
        final = await installed_set(ws)
        if final is None:
            raise SystemExit(1)
        if ORPHAN in final:
            bad(f"{ORPHAN} was installed even though its closure was refused")
        else:
            ok(f"{ORPHAN} is not installed")

        step("6. INSTALLING AGAIN IS NOT A SECOND COPY")
        # Every dependency is already present at the resolved version, so the run must report them
        # as present rather than fetching and replacing them.
        result, error = await rpc(
            ws, MARKETPLACE, "install", [APP, None, "Interactive", None, REGISTRY_NAME]
        )
        if error:
            bad(f"a repeat install refused: {error}")
        else:
            show("repeat install report", result)
            report = result or {}
            present = report.get("already_present") or []
            installed_again = [
                x.get("fqid") if isinstance(x, dict) else x for x in (report.get("installed") or [])
            ]
            if present and not installed_again:
                ok(f"all {len(present)} were recognised as already present")
            else:
                bad(f"a repeat install re-installed {installed_again}")

    print("\n" + ("=" * 70))
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mthe closure-install leg passed\033[0m")


asyncio.run(main())
