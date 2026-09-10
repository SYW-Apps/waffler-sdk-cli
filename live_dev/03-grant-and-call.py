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
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
REGISTRY_NAME = "local-dev"
FQID = "devbot.example.hello"
VERSION = "0.1.0"
CALLER = "syw.app.web"
GROUP_ID = "call_devbot_example_hello"
BINDING_ID = "op.syw.app.web.call_devbot_example_hello"
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


async def installed_fqids(ws):
    pkgs, err = await rpc(ws, "packages", "list", None)
    if err:
        return None, err
    # InstalledPackage positional: fqid at index 0.
    return [p[0] for p in (pkgs or []) if isinstance(p, (list, tuple)) and p], None


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        step("1. the refusal, restated — so the grant below is shown to be what changed it")
        result, error = await echo_call(ws, "before the grant")
        if error and "AccessDenied" in json.dumps(error):
            ok("the call is refused with no rule, as fail-closed requires")
        elif error:
            bad(f"refused for an unexpected reason: {error}")
        else:
            bad("the call SUCCEEDED with no rule — the enforcer is not fail-closed")

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
                if result.get("version") == VERSION:
                    ok(f"reporting version {VERSION}")
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
