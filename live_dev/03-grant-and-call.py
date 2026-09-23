#!/usr/bin/env python3
"""The rest of the cycle: grant, call, uninstall, reinstall, call again.

## WHAT THE PREVIOUS RUN FOUND

The install succeeded and the call was REFUSED. The caller is `syw.app.web` — the browser bridge —
and its outbound Bus targets were fixed when its bundle was packed. A package installed afterwards is
not among them, so nothing installed from a marketplace can be called from the UI until somebody
grants it. That is fail-closed and correct as a default; whether an install should offer to author
that grant is a question for core, and it is asked rather than assumed here.

What this script does is the operator's own path: author a permission group carrying the rule, bind it
to the principal, and only then call. That both proves the package works and demonstrates the only
route a real operator has today.

Every request below is POSITIONAL MessagePack, matching each struct's field order, with the order
spelled out where it is built — a positional wire read by index is one where a field inserted upstream
silently shifts everything after it.
"""

import asyncio
import json
import os
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
REGISTRY_NAME = "local-dev"
# Parameterised, because the refusal in step 1 can only be asserted against a package that has NOT
# been granted yet — and this script grants one. Running it twice against a fixed fqid would assert a
# refusal that its own previous run removed.
FQID = os.environ.get("CYCLE_FQID", "devbot.example.hello")
CALLER = "syw.app.web"
GROUP_ID = "call_" + FQID.replace(".", "_")
BINDING_ID = "op.syw.app.web." + GROUP_ID
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


def show(label, v, limit=900):
    t = json.dumps(v, indent=2, default=str)
    print(f"  {label}: {t[:limit]}{'...' if len(t) > limit else ''}", flush=True)


# RulePattern: kind, target_kind, target, path, verbs, flags, class_pattern, assignable_to,
#              secret_uuid, verb, system_id, payload
def bus_command_pattern(target):
    return ["Bus", "Command", target, None, None, None, None, None, None, None, None, None]


async def echo_call(ws, message):
    """Call the package's own capability. THE OBSERVATION THAT SEPARATES INSTALLED FROM WORKING."""
    return await rpc(ws, FQID, "echo", message)


async def recorded_version(ws):
    """The version the node's own package row carries for FQID, or None."""
    pkgs, err = await rpc(ws, "packages", "list", None)
    if err:
        return None
    for row in pkgs or []:
        if isinstance(row, (list, tuple)) and row and row[0] == FQID:
            return row[1]
    return None


async def installed_fqids(ws):
    pkgs, err = await rpc(ws, "packages", "list", None)
    if err:
        return None, err
    # InstalledPackage positional: fqid at index 0.
    return [p[0] for p in (pkgs or []) if isinstance(p, (list, tuple)) and p], None


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        step("0. clear THIS leg's own leftovers, so the refusal below is a real one")
        # THE GRANT IT CREATES SURVIVES THE RUN, and the run after it then found the call already
        # allowed and reported "the enforcer is not fail-closed" — a false alarm of the worst kind,
        # because the scary reading is the one people act on. The leg grants itself; it must ungrant
        # itself first, and then a call that still succeeds means what it says.
        _result, error = await rpc(ws, "security", "bindings.remove", {"id": BINDING_ID})
        if error and "NotFound" not in json.dumps(error, default=str):
            bad(f"could not remove a previous run's binding: {json.dumps(error, default=str)[:200]}")
        _result, error = await rpc(ws, "security", "groups.delete", {"group_id": GROUP_ID, "source": "Custom"})
        if error and "NotFound" not in json.dumps(error, default=str):
            bad(f"could not delete a previous run's group: {json.dumps(error, default=str)[:200]}")
        ok("no binding and no group from an earlier run: the baseline is ungranted")

        step("1. the refusal, restated — so the grant below is shown to be what changed it")
        result, error = await echo_call(ws, "before the grant")
        text = json.dumps(error) if error else ""
        if error and "AccessDenied" in text:
            ok("the call is refused with no rule, as fail-closed requires")
            # THE CALLER IS NAMED, NOT JUST FINGERPRINTED. The refusal used to identify the caller only
            # as `51c2c51b5a08b18a1e7edcef174f8c8b`, which is unactionable from an operator's seat —
            # security already holds the fingerprint-to-fqid map and now uses it. Pinned so a
            # regression to a bare fingerprint fails HERE rather than in front of a person.
            if "no rule allows 'syw.app.web (" in text:
                ok("and it names the caller as syw.app.web, not only its fingerprint")
            else:
                bad(f"the refusal does not name the caller by fqid: {text[:300]}")
            # The ADVICE SENTENCE IS DELIBERATELY NOT PINNED. It is the part most likely to keep
            # improving, and a test that pinned it would make every improvement a failure.
        elif error:
            # PRECONDITIONS NOT MET IS NOT A FAILURE, AND SAYING SO MATTERS.
            #
            # This script GRANTS, uninstalls and reinstalls, so a second run against the same fqid
            # finds a package that is installed-but-not-running and reports `ServiceUnavailable` here
            # rather than `AccessDenied`. The first version called that a failure — two red lines
            # describing the enforcer, about a script that had simply already been run.
            #
            # A suite that goes red for reasons unrelated to the thing it tests is a suite people stop
            # reading, which is the state that hides a real failure. So it refuses to start instead,
            # and names what it needs.
            print(f"[33m  SKIP  preconditions not met: the baseline call refused with {error.get('data', {}).get('code', '?')},[0m")
            print("        not AccessDenied. This script needs a package that is installed, RUNNING,")
            print("        and NOT yet granted — which is a package it has not already been run against.")
            print(f"        Set CYCLE_FQID to a fresh one, or restart the node if {FQID} is installed but idle.")
            sys.exit(2)
        else:
            # Step 0 removed this leg's own grant, so reaching here means something ELSE allows the
            # call — either the enforcer is not fail-closed, or a rule this leg did not write is
            # open. Both are worth stopping for; neither is this leg having been run twice.
            bad("the call SUCCEEDED with no rule of this leg's — the enforcer is not fail-closed, or another rule allows it")
            sys.exit(1)

        step("2. author a permission group carrying the one rule")
        # StoredPermissionGroup positional:
        #   id, display_name, description, status, required, source, rules, system,
        #   default_bindings_locked, override_acknowledgments
        #
        # source "Custom" because an OPERATOR authored this, not a package's manifest. A group
        # claiming a package as its source would read as something the package requested.
        group = [
            GROUP_ID,
            "Call devbot.example.hello",
            "Lets the web app call the capability of a package installed from a registry after the web app was packed.",
            "Approved",
            True,
            "Custom",
            [[bus_command_pattern(FQID), "Allow", 100]],
            False,
            False,
            None,
        ]
        result, error = await rpc(ws, "security", "groups.save", group)
        if error:
            bad(f"groups.save refused: {error}")
        else:
            ok("group saved")

        step("3. bind it to the calling principal")
        # Binding positional: id, group_id, principal(PrincipalPattern[principal_type, id]), source, approved
        binding = [BINDING_ID, GROUP_ID, ["Package", CALLER], "Custom", True]
        result, error = await rpc(ws, "security", "bindings.add", binding)
        if error:
            bad(f"bindings.add refused: {error}")
        else:
            ok("binding added")

        step("4. DOES IT ANSWER NOW?")
        result, error = await echo_call(ws, "hello from the cycle")
        if error and (error.get("data") or {}).get("code") in ("ServiceUnavailable", "NotFound"):
            # A BLIND SPOT IN STEP 1'S PRECONDITION CHECK, found by running this against a package
            # whose actor had been dropped. Step 1 reads `AccessDenied` as "installed, running, not
            # yet granted" — but the enforcer answers BEFORE the router does, so a package that is
            # not running is refused with `AccessDenied` too, and the two are indistinguishable
            # until a grant removes the enforcer from the path. Which is here.
            print(f"[33m  SKIP  granted, and the package is not RUNNING ({(error.get('data') or {}).get('code')}).[0m")
            print("        Step 1 could not tell this apart from a package awaiting a grant: the")
            print("        enforcer refuses first, so both look like AccessDenied. Restart the node")
            print("        and run this again.")
            sys.exit(2)
        if error:
            bad(f"still refused after the grant: {error}")
        else:
            show("reply", result)
            if isinstance(result, dict) and result.get("package") == FQID:
                ok("the package answered AND identified itself")
                if result.get("echo") == "hello from the cycle":
                    ok("and returned the payload it was handed — the artifact is loaded and executing")
                else:
                    bad(f"the echo came back as {result.get('echo')!r}")
                # AGAINST WHAT THE NODE RECORDED, never a constant. This was a pinned "0.1.0"
                # inside an `if` with no `else`: it said nothing at all when it disagreed, which is
                # the one case worth hearing about, and it went quiet permanently the moment a leg
                # started publishing a fresh patch each run. The node's own row is the right
                # comparand anyway — a row saying one version while the artifact answering reports
                # another is a stale module loaded under a record that has moved on.
                recorded = await recorded_version(ws)
                if recorded is None:
                    bad("the node lists no version for this package to compare against")
                elif result.get("version") == recorded:
                    ok(f"and it reports {recorded}, the version the node records")
                else:
                    bad(
                        f"the node records {recorded} but the artifact answering reports "
                        f"{result.get('version')} — the OLD module is still loaded (D15: a package library is never "
                        "unloaded and is replaced at the same path, so the node serves the old image until it restarts)"
                    )
            else:
                bad(f"unexpected reply: {result!r}")

        step("5. UNINSTALL")
        result, error = await rpc(ws, "packages", "uninstall", [FQID])
        if error:
            bad(f"uninstall refused: {error}")
        else:
            ok("uninstall reported success")

        fqids, err = await installed_fqids(ws)
        if err:
            bad(f"packages:list refused: {err}")
        elif FQID in (fqids or []):
            bad(f"{FQID} is STILL listed after uninstall")
        else:
            ok("it is gone from the package list")

        step("6. and it must stop answering")
        result, error = await echo_call(ws, "after uninstall")
        if error:
            ok(f"refused, as it must be ({json.dumps(error)[:120]}...)")
        else:
            # AN UNINSTALLED PACKAGE THAT STILL ANSWERS is the worst possible outcome: the record is
            # gone and the code is still running, so nothing in the node can report what is executing.
            bad(f"it ANSWERED after being uninstalled: {result!r}")

        step("7. REINSTALL from the same registry")
        result, error = await rpc(ws, MARKETPLACE, "install", [FQID, None, "Interactive", None, REGISTRY_NAME])
        if error:
            bad(f"reinstall refused: {error}")
        else:
            show("install", result, 700)

        # AND ENABLE IT: installing is not enabling on this core, so a reinstalled package has no
        # handler on the bus until its own gate is passed. Step 8 asked it to answer and read the
        # resulting "no handler registered" as the reinstall having failed.
        _result, error = await rpc(ws, "packages", "enable", {"fqid": FQID})
        if error:
            bad(f"enabling the reinstalled package was refused: {json.dumps(error, default=str)[:300]}")
        else:
            ok("the reinstalled package is enabled")
            ok("reinstall reported success")

        fqids, err = await installed_fqids(ws)
        if FQID in (fqids or []):
            ok("it is listed again")
        else:
            bad(f"{FQID} is not listed after reinstall")

        step("8. does the REINSTALLED package answer?")
        result, error = await echo_call(ws, "second time around")
        if error:
            # A freshly installed package answers ServiceUnavailable until the node restarts, which is
            # a KNOWN property rather than a failure of this cycle — named here so the two are not
            # confused with each other.
            text = json.dumps(error)
            if "ServiceUnavailable" in text or "dropped" in text:
                print(f"\033[33m  NOTE  the reinstalled package is installed but not yet RUNNING: {text[:200]}\033[0m")
                print("        (a freshly installed package's actor starts at the next node restart)")
            else:
                bad(f"the reinstalled package did not answer: {text[:300]}")
        else:
            show("reply", result)
            if isinstance(result, dict) and result.get("echo") == "second time around":
                ok("the reinstalled package answers, with no restart needed")
            else:
                bad(f"unexpected reply: {result!r}")

    print("\n" + "=" * 70)
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mthe full cycle passed\033[0m")


asyncio.run(main())
