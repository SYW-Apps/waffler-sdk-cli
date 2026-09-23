#!/usr/bin/env python3
"""THE POST-INSTALL APPROVAL DOOR, on the live node: three asks left pending, and what opens each.

Run `21-approval-door-publish.sh` first. It publishes

       devbot.mwa.gate  — one REQUIRED permission group (`ping`), one fast-lane request, one
                          REQUIRED middleware declaration (`gate`, Observing, scoped to itself).

An AUTOMATED install commits without answers: nothing is approved and nothing is refused, so every
ask lands Pending. That is the state an operator's approval page exists for, and the verb behind that
page is `packages:approve_requests {fqid, group_ids, fast_lane_targets, middleware_ids}`.

WHAT THIS PROVES, in the order that makes each step mean something:
  1. the preview carries the middleware row — id, kind, scope and the DIGEST an approval binds to —
     beside the group and the lane, so an operator can see all three before any byte is installed;
  2. an Automated install commits with all three unanswered: no approved group, no lane grant, a
     PENDING review already bound to the previewed digest, and the host's own grant group present
     in the security catalog but unapproved — created at ingest, so approving it later is an
     ordinary group flip rather than a group conjured at approval time;
  3. enable REFUSES (`PermissionUnapproved`), naming the group;
  4. approving only the group is not enough — enable refuses again, now `MiddlewareUnapproved`.
     TWO SEPARATE DOORS, which is the whole point: a package cannot reach the bus chain on the
     strength of an operator having approved something else;
  5. approving a declaration the package does not have is refused, naming what it does declare;
  6. approving the lane and the declaration records a review BOUND TO THE DIGEST THE PREVIEW SHOWED,
     materializes the lane grant, and flips the host's own `host.middleware_grant` group in the
     SECURITY CATALOG — the grant a package may never declare for itself, and not a field on the
     package record, so it is read where it actually lives;
  7. and only then does enable succeed.
The refusals come first: an enable that succeeds proves nothing unless the ones that must fail do.
"""

import asyncio
import json
import sys

import msgpack
import websockets

NODE = "ws://waffler:42069/api/ws"
REGISTRY_NAME = "local-dev"
MARKETPLACE = "syw.system.marketplace"

FQID = "devbot.mwa.gate"
GROUP = "ping"
LANE = "syw.system.diagnostics"
MW = "gate"
HOST_GRANT_GROUP = "host.middleware_grant"  # waffler_shared::MIDDLEWARE_GRANT_GROUP_ID

# `packages:list` is POSITIONAL; see 12-install-closure.py for why this is a set of known arities.
PACKAGE_ARITIES = {16, 17}
VERSION_AT, ENABLED_AT, LANE_GRANTS_AT, APPROVED_GROUPS_AT, REVIEWS_AT = 1, 10, 12, 13, 16
# MiddlewareReview, positional: id, review, consent_digest, approved_scope.
REVIEW_ID_AT, REVIEW_STATE_AT, REVIEW_DIGEST_AT = 0, 1, 2
# StoredPermissionGroup, positional: id, display_name, description, status, required, source, ...
GROUP_STATUS_AT, GROUP_SOURCE_AT = 3, 5

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


async def record(ws):
    """The node's own row for the fixture, or None. What a record says, never what a report says."""
    result, error = await rpc(ws, "packages", "list", None)
    if error:
        bad(f"packages:list refused: {error}")
        return None
    for row in result or []:
        if not isinstance(row, (list, tuple)) or len(row) not in PACKAGE_ARITIES:
            bad(f"an installed package arrived with an unexpected arity: {row!r}")
            return None
        if row[0] == FQID:
            return row
    return None


def reviews_of(row):
    """The middleware reviews on a record. A record from before the field simply has none."""
    return list(row[REVIEWS_AT]) if len(row) > REVIEWS_AT and row[REVIEWS_AT] else []


def review_for(row, mw_id):
    for r in reviews_of(row):
        if r[REVIEW_ID_AT] == mw_id:
            return r
    return None


async def host_grant(ws):
    """(status, source) of the host's middleware-grant group FOR THIS PACKAGE, or (None, None).

    ALWAYS SCOPED BY SOURCE. Every package that declares middleware gets a group under this one id,
    so a bare lookup answers with SOME package's copy — asked without a source on this node, it
    answered with another package's, Pending, which would have read here as "not yet approved" about
    a package this leg never touched.
    """
    result, error = await rpc(ws, "security", "groups.get", {"group_id": HOST_GRANT_GROUP, "source": FQID})
    if error or not result:
        return None, None
    return result[GROUP_STATUS_AT], result[GROUP_SOURCE_AT]


async def expect_enable_refused(ws, code, when):
    """Enable must refuse, and refuse WITH the code that names which door is shut."""
    result, error = await rpc(ws, "packages", "enable", {"fqid": FQID})
    if not error:
        bad(f"{when}: enable SUCCEEDED — {json.dumps(result, default=str)[:300]}")
        return ""
    message = json.dumps(error, default=str)
    if code in message:
        ok(f"{when}: refused as {code}")
    else:
        bad(f"{when}: refused, but not as {code}: {message[:400]}")
    return message


async def main():
    async with websockets.connect(NODE, max_size=64 * 1024 * 1024) as ws:
        step("0. the node holds the registry, and holds no copy of the fixture")
        result, error = await rpc(ws, MARKETPLACE, "registries.list", None)
        if error:
            bad(f"registries.list refused: {error}")
            raise SystemExit(1)
        if REGISTRY_NAME not in [r.get("name") for r in (result or [])]:
            print(f"\033[31m  {REGISTRY_NAME} is not registered. Run 02-install.py first.\033[0m")
            raise SystemExit(2)
        ok(f"{REGISTRY_NAME} is registered")

        # THIS LEG OWNS ITS STARTING STATE. A leftover copy would be installed and possibly MOUNTED,
        # and core's enable returns Ok early for a mounted package without persisting the flag — so
        # every refusal below would be measuring the last run rather than this one.
        _result, error = await rpc(ws, "packages", "uninstall", {"fqid": FQID})
        if error and "NotFound" not in json.dumps(error, default=str):
            bad(f"could not clear a previous run's {FQID}: {json.dumps(error, default=str)[:300]}")
        if await record(ws) is not None:
            bad(f"a previous run's {FQID} is still installed")
            raise SystemExit(1)
        ok(f"the node holds no {FQID}: this run starts from nothing")

        step("1. THE PREVIEW carries the middleware row, with the digest an approval binds to")
        preview, error = await rpc(ws, MARKETPLACE, "preview_closure", [FQID, None, REGISTRY_NAME])
        if error:
            bad(f"preview_closure refused: {error}")
            raise SystemExit(1)
        previewed = {p.get("namespace"): p for p in (preview or {}).get("previewed", [])}
        if FQID not in previewed:
            # REFUSES TO START rather than reporting failures: with nothing previewed there is no
            # version to install, and every step below would pass vacuously.
            print(f"\033[31m  {FQID} is not previewed (already_present={(preview or {}).get('already_present')}). "
                  f"Run 21-approval-door-publish.sh.\033[0m")
            raise SystemExit(2)
        entry = previewed[FQID]
        version = entry.get("version")
        requests = entry.get("requests") or {}
        previewed_digest = None
        rows = requests.get("middleware") or []
        row = next((m for m in rows if m.get("id") == MW), None)
        if row is None:
            bad(f"the preview shows no middleware '{MW}': {json.dumps(requests, default=str)[:400]}")
        else:
            previewed_digest = row.get("consent_digest")
            if row.get("required") is True and row.get("kind") == "Observing" and previewed_digest:
                ok(f"{FQID}@{version} previews middleware '{MW}': required, Observing, digest {previewed_digest[:16]}…")
            else:
                bad(f"the middleware row is not what 21 published: {json.dumps(row, default=str)[:400]}")
            # The SCOPE is what the digest is over, so an operator who cannot see it cannot consent to it.
            targets = (((row.get("scope") or {}).get("commands") or {}).get("targets") or {}).get("any_of")
            if targets == [FQID]:
                ok(f"and it carries the scope whole: commands addressed to {FQID}")
            else:
                bad(f"the previewed scope is not the declared one: {targets!r}")
        if any(g.get("id") == GROUP and g.get("required") for g in requests.get("permission_groups") or []):
            ok(f"beside the required group '{GROUP}'")
        else:
            bad(f"the preview does not show the required group '{GROUP}'")
        if any(l.get("target") == LANE for l in requests.get("fast_lane_requests") or []):
            ok(f"and the fast-lane request for {LANE}")
        else:
            bad(f"the preview does not show the fast-lane request for {LANE}")

        step("2. AN AUTOMATED INSTALL commits with all three unanswered")
        result, error = await rpc(ws, MARKETPLACE, "install", [FQID, None, "Automated", None, REGISTRY_NAME])
        if error:
            bad(f"the Automated install was refused: {json.dumps(error, default=str)[:400]}")
            raise SystemExit(1)
        row = await record(ws)
        if row is None:
            bad(f"{FQID} is not installed after the install reported {json.dumps(result, default=str)[:200]}")
            raise SystemExit(1)
        ok(f"{FQID}@{row[VERSION_AT]} is installed")
        if row[ENABLED_AT] is False:
            ok("and it is NOT enabled: installing is not enabling")
        else:
            bad(f"it installed enabled={row[ENABLED_AT]!r}")
        if GROUP not in (row[APPROVED_GROUPS_AT] or []):
            ok(f"no approval of '{GROUP}': an Automated run approves nothing it was not told to")
        else:
            bad(f"'{GROUP}' is approved after an install that answered nothing: {row[APPROVED_GROUPS_AT]}")
        if not (row[LANE_GRANTS_AT] or []):
            ok("no fast-lane grant: a request is an ASK, and only an approval produces a lane")
        else:
            bad(f"a lane grant exists without an approval: {row[LANE_GRANTS_AT]}")
        review = review_for(row, MW)
        if review is None:
            # AN INSTALL RECORDS THE ASK. Absence would mean the declaration is invisible to anything
            # reading the record — an operator page could not offer what it cannot see.
            bad(f"'{MW}' has no review at all: the ask is invisible to anything reading the record")
        elif review[REVIEW_STATE_AT] == "Pending":
            ok(f"and '{MW}' is recorded PENDING: presented, unanswered, and not silently approved")
            if previewed_digest and review[REVIEW_DIGEST_AT] == previewed_digest:
                ok("with the digest the preview showed already bound to it")
            elif previewed_digest:
                bad(f"the pending review binds {review[REVIEW_DIGEST_AT]!r}, the preview showed {previewed_digest!r}")
        else:
            bad(f"'{MW}' installed already reviewed {review[REVIEW_STATE_AT]!r}")

        # THE HOST'S OWN GRANT GROUP EXISTS FROM INSTALL, UNAPPROVED. It is created by the ingest
        # because the package declares middleware, so approving it later is an ordinary group flip
        # rather than a group being conjured at approval time — and an author may never declare it.
        status, source = await host_grant(ws)
        if status is None:
            bad(f"the security catalog has no '{HOST_GRANT_GROUP}' for {FQID}")
        elif status != "Approved" and source == FQID:
            ok(f"the host's '{HOST_GRANT_GROUP}' exists for {FQID} and is {status}, not approved")
        else:
            bad(f"'{HOST_GRANT_GROUP}' for {FQID} is already {status!r} (source {source!r})")

        step("3. ENABLE REFUSES while the required group is unapproved")
        message = await expect_enable_refused(ws, "PermissionUnapproved", "nothing approved")
        if message and GROUP not in message:
            bad(f"the refusal does not name '{GROUP}': {message[:400]}")

        step("4. APPROVING THE GROUP IS NOT ENOUGH — the middleware door is a second door")
        _result, error = await rpc(
            ws, "packages", "approve_requests",
            {"fqid": FQID, "group_ids": [GROUP], "fast_lane_targets": [], "middleware_ids": []},
        )
        if error:
            bad(f"approving the group was refused: {json.dumps(error, default=str)[:400]}")
        row = await record(ws)
        if row is not None:
            if GROUP in (row[APPROVED_GROUPS_AT] or []):
                ok(f"'{GROUP}' is approved on the record")
            else:
                bad(f"'{GROUP}' is not approved after approve_requests: {row[APPROVED_GROUPS_AT]}")
            if not (row[LANE_GRANTS_AT] or []):
                ok("and the lane it did not name is still ungranted")
            else:
                bad(f"approving a GROUP produced a lane grant: {row[LANE_GRANTS_AT]}")
            review = review_for(row, MW)
            if review is not None and review[REVIEW_STATE_AT] == "Pending":
                ok("and the declaration it did not name is still Pending")
            else:
                bad(f"approving a GROUP moved the middleware review: {review!r}")
        message = await expect_enable_refused(ws, "MiddlewareUnapproved", "with only the group approved")
        if message and MW not in message:
            bad(f"the refusal does not name '{MW}': {message[:400]}")

        step("5. A DECLARATION THE PACKAGE DOES NOT HAVE is refused, naming what it does")
        _result, error = await rpc(
            ws, "packages", "approve_requests",
            {"fqid": FQID, "group_ids": [], "fast_lane_targets": [], "middleware_ids": ["not-declared"]},
        )
        if not error:
            bad("approving a declaration the package never made was accepted")
        else:
            message = json.dumps(error, default=str)
            if "NotFound" in message and MW in message:
                ok(f"refused as NotFound, naming the one it does declare ('{MW}')")
            else:
                bad(f"refused, but not as a NotFound naming '{MW}': {message[:400]}")
        row = await record(ws)
        review = review_for(row, MW) if row is not None else None
        if review is not None and review[REVIEW_STATE_AT] == "Pending":
            ok(f"and it changed nothing: '{MW}' is still Pending")
        else:
            bad(f"the refused call moved the review anyway: {review!r}")

        step("6. APPROVING THE LANE AND THE DECLARATION opens both")
        _result, error = await rpc(
            ws, "packages", "approve_requests",
            {"fqid": FQID, "group_ids": [], "fast_lane_targets": [LANE], "middleware_ids": [MW]},
        )
        if error:
            bad(f"approving the lane and the declaration was refused: {json.dumps(error, default=str)[:400]}")
        row = await record(ws)
        if row is not None:
            grants = row[LANE_GRANTS_AT] or []
            if any((g[0] if isinstance(g, (list, tuple)) else g.get("target")) == LANE for g in grants):
                ok(f"a fast-lane grant for {LANE} now exists")
            else:
                bad(f"no lane grant for {LANE} after approving it: {grants!r}")
            review = review_for(row, MW)
            if review is None:
                bad(f"'{MW}' still has no review after being approved: {reviews_of(row)!r}")
            else:
                if review[REVIEW_STATE_AT] == "Approved":
                    ok(f"'{MW}' is reviewed Approved")
                else:
                    bad(f"'{MW}' is reviewed {review[REVIEW_STATE_AT]!r}, not Approved")
                # THE POINT OF A DIGEST: the approval is bound to the declaration the operator SAW.
                # A widened scope changes the digest, and a review that no longer matches approves
                # nothing — so a review carrying some other digest would be consent to the wrong thing.
                if previewed_digest and review[REVIEW_DIGEST_AT] == previewed_digest:
                    ok("and it is bound to the digest the preview showed, not to a re-read of the manifest")
                elif previewed_digest:
                    bad(f"the review binds {review[REVIEW_DIGEST_AT]!r}, the preview showed {previewed_digest!r}")
        # THE HOST GRANT IS FLIPPED IN THE SECURITY CATALOG, not listed on the package record: the
        # record carries the AUTHOR's approved group ids, and a grant the host derives is not one of
        # them. Reading it where it actually lives is the difference between proving the grant and
        # proving a field.
        status, source = await host_grant(ws)
        if status == "Approved" and source == FQID:
            ok(f"and the host's '{HOST_GRANT_GROUP}' for {FQID} is now Approved — the grant a package may never declare")
        else:
            bad(f"'{HOST_GRANT_GROUP}' for {FQID} is {status!r} (source {source!r}) after approving '{MW}'")

        step("7. AND ONLY NOW does enable succeed")
        result, error = await rpc(ws, "packages", "enable", {"fqid": FQID})
        if error:
            bad(f"enable was still refused: {json.dumps(error, default=str)[:400]}")
        else:
            ok("enable accepted")
        row = await record(ws)
        if row is not None and row[ENABLED_AT] is True:
            ok(f"{FQID}@{row[VERSION_AT]} is enabled, with every ask answered")
        else:
            bad(f"{FQID} is not enabled after the enable: {row!r}")

    print("\n" + "=" * 70)
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mthe approval door holds: three asks, three answers, and no shortcut between them\033[0m")


asyncio.run(main())
