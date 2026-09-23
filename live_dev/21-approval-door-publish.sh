#!/bin/bash
# THE POST-INSTALL APPROVAL DOOR — publishes a package that installs with things left PENDING.
#
# An Automated install commits without answers: its required permission group, its fast-lane request
# and its middleware declaration all land UNANSWERED. That is the state an operator's approval page
# exists for, and `22-approval-door.py` drives it with `packages:approve_requests`.
#
#        devbot.mwa.gate  — one required permission group (`ping`), one fast-lane request, one
#                           REQUIRED middleware declaration.
#
# WHY THE DECLARATION IS `Observing` AND SCOPED TO ITS OWN SERVICE. An approved declaration is
# registered on the GLOBAL bus chain, so a fixture that could reject messages is a fixture that can
# wedge a shared node. `Observing` cannot stop or alter a message, and the scope admits only commands
# addressed to the fixture itself — nothing else on the node ever passes through it. The leg is about
# WHO OPENS THE DOOR, not about what an interceptor may do once through it.
#
# THE GROUP DELIBERATELY DOES NOT ASK FOR `bus:register_middleware`. The CLI advises against it and is
# right: this core grants that verb through the host's own `host.middleware_grant`, approved with the
# operator's review of the declaration, so a package-authored rule for it admits no layer. The group
# here is the harmless diagnostics ping 18 uses — it exists to give the GROUP door something to
# refuse, which is what makes 22 able to show the group and middleware doors are separate.
set -uo pipefail

RED()  { printf '\033[31m%s\033[0m\n' "$*"; }
OK()   { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
TAG="devbot"
FQID="devbot.mwa.gate"
# A FRESH PATCH EVERY RUN, for the reason 18 documents: republishing a version is refused (409), and
# 22 needs a version the node does not already hold.
VERSION="0.1.$(date +%s)"

STEP "0. build the CLI"
cd /work/sdk/cli || exit 1
cargo build --locked --quiet 2>&1 | tail -20
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"
[[ -x "$WAFFLER" ]] || { RED "the CLI did not build"; exit 1; }

STEP "1. mint a developer token and claim the tag"
TOKEN=$(curl -s --max-time 10 "$IDP/token?username=devbot&entitlements=waffler-developer" | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')
[[ -n "$TOKEN" ]] || { RED "no token"; exit 1; }
export WAFFLER_REGISTRY_TOKEN="$TOKEN"
curl -s --max-time 10 -X POST "$REGISTRY/v1/developer/namespaces" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"tag\":\"$TAG\"}" -o /dev/null -w 'claim http=%{http_code}\n'

STEP "2. author the package: a required group, a fast lane, a required middleware"
PROJECT="/tmp/approval-door-${VERSION//./_}"
rm -rf "$PROJECT"
"$WAFFLER" scaffold "$FQID" --path "$PROJECT" --version "$VERSION" --sdk-path /work >/dev/null || { RED "scaffold failed"; exit 1; }

FQID="$FQID" python3 - "$PROJECT/waffler.json" <<'PY'
import json, os, sys
path, fqid = sys.argv[1], os.environ['FQID']
manifest = json.load(open(path))

# CORE'S GROUP SCHEMA IN FULL, for the reason 18 gives: a field missing here is found by the node at
# install, and a leg that failed there would be reporting a malformed fixture as an approval defect.
manifest['permissionGroups'] = [{
    "id": "ping",
    "display_name": "Ask the diagnostics service for a ping",
    "description": "approval-door fixture: one REQUIRED group, so the group door has something to refuse",
    "status": "PendingReview",
    "required": True,
    "source": fqid,
    "rules": [{
        "pattern": {"kind": "Bus", "target_kind": "Command", "target": "syw.system.diagnostics", "verb": "ping"},
        "effect": "Allow",
        "priority": 100,
    }],
    "system": False,
    "default_bindings_locked": False,
}]

# THE ASK, never a grant: a request names a target, and only an approval on the node produces a lane.
manifest['fastLaneRequests'] = [{"target": "syw.system.diagnostics", "secure": False}]

# Observing, and scoped to its own service. See the header.
manifest['middleware'] = [{
    "id": "gate",
    "handler": "gate",
    "kind": "Observing",
    "required": True,
    "scope": {"commands": {"targets": {"any_of": [fqid]}}},
}]

open(path, 'w').write(json.dumps(manifest, indent=2))
PY

cd "$PROJECT" || exit 1
"$WAFFLER" validate >/dev/null || { RED "$FQID does not validate"; "$WAFFLER" validate; exit 1; }
"$WAFFLER" pack >/dev/null 2>&1 || { RED "$FQID did not pack"; exit 1; }
"$WAFFLER" publish --registry "$REGISTRY" >/dev/null 2>&1 || { RED "$FQID did not publish"; exit 1; }
OK "published $FQID@$VERSION"

STEP "3. the registry carries the asks it models on that version"
# ASSERTED AT THE REGISTRY, not assumed from the pack. 22 proves that approving each one opens a
# door; if an ask were dropped between the manifest and the catalog there would be no door to open,
# and every refusal 22 expects would be absent for the wrong reason.
# THE BODY TRAVELS IN THE ENVIRONMENT, not on stdin: a heredoc IS stdin, so piping curl into a
# script written this way hands the reader the script's own text and it decodes nothing.
BODY=$(curl -s --max-time 10 "$REGISTRY/v1/packages/$FQID/versions/$VERSION") python3 - <<'PY'
import json, os, sys
d = json.loads(os.environ["BODY"] or "{}")
problems = []
groups = d.get("permission_groups") or []
if not any(g.get("id") == "ping" and g.get("required") is True for g in groups):
    problems.append(f"no required group 'ping': {groups}")
lanes = d.get("fast_lane_requests") or []
if not any(l.get("target") == "syw.system.diagnostics" for l in lanes):
    problems.append(f"no fast-lane request: {lanes}")
# THE CATALOG CARRIES NO MIDDLEWARE, and that is a gap rather than an expectation: the registry reads
# the declarations only to set a `declares_middleware` flag for taxonomy, while it stores and serves
# permission groups and lane requests in full. So a package page can show two of the three asks. The
# consent path does not depend on it (the marketplace previews the STAGED BUNDLE through core, which
# does report middleware with its digest), which is why 22 can prove the door today. Asserted here as
# soon as the registry carries it.
if d.get("middleware"):
    print("NOTE: the catalog now carries middleware; assert it here and drop this note")
for p in problems:
    print(f"\033[31m{p}\033[0m")
sys.exit(1 if problems else 0)
PY
fail=$?
[[ $fail -eq 0 ]] && OK "the catalog carries the group and the lane request; run 22-approval-door.py" \
                  || RED "the fixture is not what 22 needs"
exit $fail
