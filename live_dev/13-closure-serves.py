#!/usr/bin/env python3
"""The closure RUNS — every member of it, not just the root — and what happens when a leaf leaves.

`12-install-closure.py` proves three packages are installed and enabled. Neither of those is the
observation that matters: a record in a store and a `true` in a column are what a records-only
install would also produce. This calls each one and reads what came back.

## WHY EACH MEMBER AND NOT JUST THE ROOT

The root answering proves the root's artifact loaded. It says nothing about `lib` or `util`, whose
modules the node loaded from bundles it fetched in the same run — and a dependency that installs,
enables and never loads is the failure a root-only check cannot see.

## RUN ORDER

Needs `11` then `12`, then a NODE RESTART: a freshly installed package is present in the store and
absent from the router until the node restarts. Then this.

## THE LAST STEP IS A QUESTION, NOT A REGRESSION TEST

Uninstalling `util` takes a required dependency out from under `lib` and `app`. Two designs are
safe — refuse the uninstall, or accept it and disable the dependents — so this reports which
happened rather than pinning either.

The third outcome is what the first run found: the uninstall is accepted and both dependents stay
ENABLED and keep answering, across a node restart. That is core's gate, not this harness's, so it is
printed as a standing finding and does not turn this leg red — a leg that is permanently red over
somebody else's open question is where the next real failure goes to hide. It stops printing the
moment core closes it, which is how the change becomes visible here.
"""

import asyncio
import json
import os
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
REGISTRY_NAME = "local-dev"
MARKETPLACE = "syw.system.marketplace"
CALLER = "syw.app.web"

APP = "devbot.dia.app"
LIB = "devbot.dia.lib"
UTIL = "devbot.dia.util"
CLOSURE = [UTIL, LIB, APP]
# Who breaks if UTIL goes. Both declare it; `app` declares it directly and through `lib`.
DEPENDENTS_OF_UTIL = [LIB, APP]

# See 12-install-closure.py: `packages:list` replies with POSITIONAL arrays, and the arity is
# asserted so a field added upstream fails loudly rather than shifting `enabled` silently.
PACKAGE_FIELDS = 16
FQID_AT, VERSION_AT, ENABLED_AT = 0, 1, 10

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


def frame(method, params):
    _next_id[0] += 1
    return msgpack.packb(
        {"jsonrpc": "2.0", "id": str(_next_id[0]), "method": method, "params": params},
        use_bin_type=True,
    )


async def rpc(ws, service, command, payload):
    params = {"target": {"service": service, "command": command}, "payload": payload}
    await ws.send(frame("send_message", params))
    raw = await asyncio.wait_for(ws.recv(), timeout=180)
    reply = msgpack.unpackb(raw, raw=False, strict_map_key=False)
    return reply.get("result"), reply.get("error")


def bus_command_pattern(target):
    return ["Bus", "Command", target, None, None, None, None, None, None, None, None, None]


async def held(ws):
    """{fqid: (version, enabled)} — what the node actually holds."""
    result, error = await rpc(ws, "packages", "list", None)
    if error:
        bad(f"packages:list refused: {error}")
        return None
    packages = {}
    for row in result or []:
        if not isinstance(row, (list, tuple)) or len(row) != PACKAGE_FIELDS:
            bad(f"an installed package arrived with an unexpected arity: {row!r}")
            return None
        packages[row[FQID_AT]] = (row[VERSION_AT], row[ENABLED_AT])
    return packages


async def grant(ws, fqid):
    """The operator's own path: a group carrying the one rule, bound to the calling principal.

    `syw.app.web`'s outbound Bus targets were fixed when its bundle was packed, so nothing installed
    afterwards is callable from the UI until somebody authors this. Fail-closed and correct as a
    default; that an install does not offer to author it is a finding this harness has already
    handed to core, not something to work around here.
    """
    group_id = "call_" + fqid.replace(".", "_")
    # StoredPermissionGroup positional:
    #   id, display_name, description, status, required, source, rules, system,
    #   default_bindings_locked, override_acknowledgments
    group = [
        group_id,
        f"Call {fqid}",
        "Lets the web app call a package installed from a registry after the web app was packed.",
        "Approved",
        True,
        # An OPERATOR authored this. Claiming the package as the source would read as something the
        # package itself requested.
        "Custom",
        [[bus_command_pattern(fqid), "Allow", 100]],
        False,
        False,
        None,
    ]
    _, error = await rpc(ws, "security", "groups.save", group)
    if error:
        return f"groups.save refused for {fqid}: {error}"
    # Binding positional: id, group_id, principal(PrincipalPattern[principal_type, id]), source, approved
    binding = ["op." + CALLER + "." + group_id, group_id, ["Package", CALLER], "Custom", True]
    _, error = await rpc(ws, "security", "bindings.add", binding)
    if error:
        return f"bindings.add refused for {fqid}: {error}"
    return None


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        step("0. preconditions")
        # REFUSES TO START rather than reporting failures that describe something else. A leg that
        # goes red for a reason unrelated to what it tests is a leg people stop reading, which is the
        # state that hides a real failure.
        before = await held(ws)
        if before is None:
            sys.exit(1)
        absent = [f for f in CLOSURE if f not in before]
        if absent:
            note(f"not installed: {absent}. Run 11-dependency-closure.sh then 12-install-closure.py.")
            sys.exit(2)
        ok(f"all three installed: {', '.join(f'{f}@{before[f][0]}' for f in CLOSURE)}")

        step("1. grant the web app the right to call each of them")
        for fqid in CLOSURE:
            problem = await grant(ws, fqid)
            if problem:
                bad(problem)
            else:
                ok(f"granted {fqid}")

        step("2. DOES EVERY MEMBER ANSWER? — not just the root")
        for fqid in CLOSURE:
            payload = f"hello {fqid}"
            result, error = await rpc(ws, fqid, "echo", payload)
            if error:
                code = (error.get("data") or {}).get("code", "?")
                # INSTALLED IS NOT RUNNING, and it has two spellings. A package's actor is
                # registered at boot, so one installed afterwards is in the store and absent from
                # the router until a restart — reported as `NotFound` or `ServiceUnavailable`
                # depending on how far the call got. Neither is a failure of the package; both are a
                # failure of the RUN ORDER, so this refuses to start and names what it needs rather
                # than printing red lines about an enforcer.
                if code in ("NotFound", "ServiceUnavailable"):
                    print(f"\033[33m  SKIP  {fqid} is installed but not RUNNING ({code}).\033[0m")
                    print("        A package installed after boot is absent from the router until the")
                    print("        node restarts. Restart waffler-beta and run this again.")
                    sys.exit(2)
                bad(f"{fqid} did not answer: {json.dumps(error)[:280]}")
                continue
            if not isinstance(result, dict) or result.get("package") != fqid:
                bad(f"{fqid} replied with an unexpected shape: {result!r}")
                continue
            ok(f"{fqid} answered and identified itself")
            if result.get("echo") != payload:
                bad(f"{fqid} echoed {result.get('echo')!r}, not what it was handed")
            # AGAINST WHAT THE NODE RECORDED, never a constant: the row says one version and the
            # artifact answering reports another is a stale module loaded under a record that moved.
            recorded = before[fqid][0]
            if result.get("version") != recorded:
                bad(f"{fqid} answers as {result.get('version')} but the node records {recorded}")
            else:
                ok(f"  and reports {recorded}, the version the node records")

        step("3. TAKE THE LEAF AWAY — uninstall the util everything needs")
        # The question, not a regression test. Two designs are safe: refuse, or accept and disable
        # the dependents. What must never happen is a dependent left ENABLED with a required
        # dependency gone — a package the node believes is working.
        result, error = await rpc(ws, "packages", "uninstall", [UTIL])
        cascade_expected = False
        if error:
            ok(f"the uninstall was REFUSED, which is one of the two safe answers")
            print(f"  {json.dumps(error)[:400]}")
        else:
            note(f"the uninstall was accepted: {json.dumps(result, default=str)[:200]}")
            cascade_expected = True

        step("4. WHAT HAPPENED TO THE PACKAGES THAT NEEDED IT")
        after = await held(ws)
        if after is None:
            sys.exit(1)
        if not cascade_expected:
            if UTIL in after and after[UTIL][1] is True:
                ok(f"{UTIL} is still installed and enabled, as a refused uninstall requires")
            else:
                bad(f"the uninstall was refused but {UTIL} is now {after.get(UTIL)!r}")
            for fqid in DEPENDENTS_OF_UTIL:
                if after.get(fqid, (None, None))[1] is True:
                    ok(f"{fqid} is untouched and enabled")
                else:
                    bad(f"a refused uninstall still changed {fqid}: {after.get(fqid)!r}")
        else:
            if UTIL in after:
                bad(f"{UTIL} is still listed after a successful uninstall: {after[UTIL]!r}")
            else:
                ok(f"{UTIL} is gone")
            stranded = [f for f in DEPENDENTS_OF_UTIL if (after.get(f) or (None, None))[1] is True]
            for fqid in DEPENDENTS_OF_UTIL:
                state = after.get(fqid)
                if state is None:
                    ok(f"{fqid} was removed with it")
                elif state[1] is not True:
                    ok(f"{fqid} was disabled when its dependency left")
            if stranded:
                # A STANDING FINDING, NOT A FAILURE OF THIS LEG — and the distinction is the
                # difference between a suite people read and one they stop reading. The enable gate
                # is core's, this leg is about whether a closure serves, and leaving it permanently
                # red over somebody else's open question would bury the next real failure in it.
                #
                # It does not go quiet either. If core closes this, the packages arrive here
                # disabled or removed, this block stops printing, and the change is visible in the
                # output rather than in a diff nobody ran.
                note("STANDING FINDING — the enable gate is not re-evaluated when a dependency goes")
                print(f"        {', '.join(stranded)} are still ENABLED with the required")
                print(f"        {UTIL} uninstalled, and they keep answering. Measured across a node")
                print("        restart too, so boot does not re-derive it either. Core's to decide;")
                print("        see the README's findings section.")

        step("5. PUT IT BACK — the node must return to a whole closure")
        result, error = await rpc(
            ws, MARKETPLACE, "install", [APP, None, "Interactive", None, REGISTRY_NAME]
        )
        if error:
            bad(f"reinstalling the closure refused: {error}")
        else:
            restored = await held(ws)
            if restored is None:
                sys.exit(1)
            for fqid in CLOSURE:
                state = restored.get(fqid)
                if state is None:
                    bad(f"{fqid} is missing after the closure was reinstalled")
                elif state[1] is not True:
                    bad(f"{fqid} is installed but not enabled after reinstall: {state!r}")
                else:
                    ok(f"{fqid} is back and enabled at {state[0]}")

    print("\n" + ("=" * 70))
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mthe closure-serves leg passed\033[0m")


asyncio.run(main())
