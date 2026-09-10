#!/usr/bin/env python3
"""The install leg of the cycle, driven over the node's own bridge.

WHY THE BRIDGE RATHER THAN A BACK DOOR. `/api/ws` is the seam the browser uses, carrying JSON-RPC
2.0 frames as MessagePack. Driving anything else — a direct bus socket, a debug endpoint — would
prove that the bus works and say nothing about whether an operator can do this. Every call below is
one a person clicking in the marketplace makes.

The marketplace's REQUESTS are positional MessagePack (its portal says so per capability) and its
REPLIES are named maps. That asymmetry is deliberate on its side, so it is honoured here rather than
worked around.
"""

import asyncio
import json
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
REGISTRY_URL = "http://registry:42070"
REGISTRY_NAME = "local-dev"
FQID = "devbot.example.hello"
VERSION = "0.1.0"

MARKETPLACE = "syw.system.marketplace"

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
    """One bus command through the bridge. Returns (result, error)."""
    params = {
        # A map rather than an array: the bridge decodes this with serde, which accepts either, and a
        # map is the spelling a reader can check against the struct without counting positions.
        "target": {"service": service, "command": command},
        "payload": payload,
    }
    await ws.send(frame("send_message", params))
    raw = await asyncio.wait_for(ws.recv(), timeout=180)
    reply = msgpack.unpackb(raw, raw=False, strict_map_key=False)
    return reply.get("result"), reply.get("error")


def show(label, value, limit=1200):
    text = json.dumps(value, indent=2, default=str)
    if len(text) > limit:
        text = text[:limit] + "\n  ..."
    print(f"  {label}: {text}", flush=True)


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        step("1. add the local registry as an ADDITIONAL registry")
        # RegistryEndpoint is positional: [name, base_url, is_default]. NOT default — the point is
        # that a node can hold several and a caller names which.
        result, error = await rpc(ws, MARKETPLACE, "registries.put", [REGISTRY_NAME, REGISTRY_URL, False])
        if error:
            bad(f"registries.put refused: {error}")
        else:
            ok("registry added")

        step("2. what registries does the node hold now?")
        result, error = await rpc(ws, MARKETPLACE, "registries.list", None)
        if error:
            bad(f"registries.list refused: {error}")
        else:
            show("registries", result)
            names = [r.get("name") for r in (result or [])]
            if REGISTRY_NAME in names:
                ok(f"{REGISTRY_NAME} is listed alongside {len(names) - 1} other(s)")
            else:
                bad(f"{REGISTRY_NAME} not in {names}")

        step("3. describe it — does the node's own network permission let it reach the host?")
        result, error = await rpc(ws, MARKETPLACE, "registry.describe", [REGISTRY_NAME])
        if error:
            bad(f"registry.describe refused: {error}")
        else:
            show("describe", result)
            ok("the node reached the registry")

        step("4. search that registry for the package the CLI just published")
        result, error = await rpc(
            ws, MARKETPLACE, "search", [REGISTRY_NAME, "hello", None, None, None, None, None]
        )
        if error:
            bad(f"search refused: {error}")
        else:
            show("search", result)
            found = json.dumps(result, default=str)
            if FQID in found:
                ok(f"{FQID} is browsable from the node")
            else:
                bad(f"{FQID} not found in the search result")

        step("5. package detail")
        result, error = await rpc(ws, MARKETPLACE, "package.get", [FQID, REGISTRY_NAME])
        if error:
            bad(f"package.get refused: {error}")
        else:
            show("package", result)

        step("6. plan the install")
        result, error = await rpc(ws, MARKETPLACE, "plan", [FQID, None, REGISTRY_NAME])
        if error:
            bad(f"plan refused: {error}")
        else:
            show("plan", result)

        step("7. preview — what would be approved")
        result, error = await rpc(ws, MARKETPLACE, "preview", [FQID, VERSION, REGISTRY_NAME])
        if error:
            bad(f"preview refused: {error}")
        else:
            show("preview", result)

        step("8. INSTALL from the local registry")
        # InstallReq is positional: [namespace, range, trigger, decisions, registry].
        result, error = await rpc(
            ws, MARKETPLACE, "install", [FQID, None, "Interactive", None, REGISTRY_NAME]
        )
        if error:
            bad(f"install refused: {error}")
        else:
            show("install", result)
            ok("install reported success")

        step("9. is it in the node's package list?")
        result, error = await rpc(ws, "packages", "list", None)
        if error:
            bad(f"packages:list refused: {error}")
        else:
            listed = json.dumps(result, default=str)
            if FQID in listed:
                ok(f"{FQID} is installed")
            else:
                bad(f"{FQID} is NOT in the node's package list")
                show("packages", result, 3000)

        step("10. DOES IT ANSWER? — the observation that separates installed from working")
        # A row in the package list is what a records-only install would also produce. Calling the
        # capability and getting the right bytes back proves the ARTIFACT LOADED and is executing.
        payload = msgpack.packb("hello from the cycle", use_bin_type=True)
        result, error = await rpc(ws, FQID, "echo", msgpack.unpackb(payload, raw=False))
        if error:
            bad(f"the package did not answer: {error}")
        else:
            show("echo reply", result)
            if isinstance(result, dict) and result.get("package") == FQID:
                ok(f"{FQID} answered, and identified itself")
                if result.get("echo") == "hello from the cycle":
                    ok("and returned the payload it was handed")
                else:
                    bad(f"the echo was {result.get('echo')!r}")
            else:
                bad(f"unexpected reply shape: {result!r}")

    print("\n" + ("=" * 70))
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mthe install leg passed\033[0m")


asyncio.run(main())
