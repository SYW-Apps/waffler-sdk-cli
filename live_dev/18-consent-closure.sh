#!/bin/bash
# CONSENT FOR A WHOLE CLOSURE — publishes the shape the marketplace's old install got wrong.
#
#        devbot.cst.app ──▶ devbot.cst.lib
#
# BOTH declare a REQUIRED permission group with the SAME id, `net`. Core applies an install's decisions
# by BARE id, and the marketplace used to hand ONE decisions set to every package of a closure — so
# approving the root's `net` approved the dependency's `net` too, a request the operator was never
# shown. Two packages sharing one group id is the smallest graph where that leak is visible, which is
# why it is the graph `19-consent-install.py` installs.
#
# The group's rule is deliberately harmless (one Bus command on the diagnostics service): this pair of
# legs is about WHOSE answer reaches WHICH bundle, not about what the grant permits.
set -uo pipefail

RED()  { printf '\033[31m%s\033[0m\n' "$*"; }
OK()   { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
TAG="devbot"
# A FRESH PATCH EVERY RUN. Republishing a version is refused (409), and 19 needs versions the node does
# not already hold — an entry the node already has is not previewed, so its pre-flight would have
# nothing to refuse and the leak test would pass by testing nothing.
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

publish_one() {
  local fqid="$1"
  local deps="$2"
  local project="/tmp/consent-${fqid##*.}-${VERSION//./_}"

  rm -rf "$project"
  "$WAFFLER" scaffold "$fqid" --path "$project" --version "$VERSION" --sdk-path /work >/dev/null || return 1

  FQID="$fqid" DEPS="$deps" python3 - "$project/waffler.json" <<'PY'
import json, os, sys
path = sys.argv[1]
manifest = json.load(open(path))
manifest['dependencies'] = [{"fqid": d, "version": "^0.1.0"} for d in json.loads(os.environ['DEPS'])]
# CORE'S GROUP SCHEMA, WRITTEN OUT IN FULL. The CLI passes permission groups through unmodelled, so a
# field missing here is found by the node at install rather than by the pack — and a leg that failed
# there would be reporting a malformed fixture as a consent defect.
manifest['permissionGroups'] = [{
    "id": "net",
    "display_name": "Ask the diagnostics service for a ping",
    "description": "consent-leg fixture: BOTH packages of this closure declare this id, on purpose",
    "status": "PendingReview",
    "required": True,
    "source": os.environ['FQID'],
    "rules": [{
        "pattern": {"kind": "Bus", "target_kind": "Command", "target": "syw.system.diagnostics", "verb": "ping"},
        "effect": "Allow",
        "priority": 100,
    }],
    "system": False,
    "default_bindings_locked": False,
}]
open(path, 'w').write(json.dumps(manifest, indent=2))
PY

  cd "$project" || return 1
  "$WAFFLER" validate >/dev/null || { RED "$fqid does not validate"; "$WAFFLER" validate; return 1; }
  "$WAFFLER" pack >/dev/null 2>&1 || { RED "$fqid did not pack"; return 1; }
  "$WAFFLER" publish --registry "$REGISTRY" >/dev/null 2>&1 || { RED "$fqid did not publish"; return 1; }
  OK "published $fqid@$VERSION  deps=$deps  permission group: net (required)"
}

STEP "2. publish the closure, leaf first"
publish_one devbot.cst.lib '[]' || exit 1
publish_one devbot.cst.app '["devbot.cst.lib"]' || exit 1

STEP "3. the registry carries the group on BOTH versions"
# ASSERTED AT THE REGISTRY, not assumed from the pack. 19 means nothing if the dependency's group was
# dropped anywhere between the manifest and the catalog, because then there is no second `net` for an
# answer to leak to — and the leak test would pass for the wrong reason.
fail=0
for fqid in devbot.cst.lib devbot.cst.app; do
  body=$(curl -s --max-time 10 "$REGISTRY/v1/packages/$fqid/versions/$VERSION")
  if printf '%s' "$body" | python3 -c 'import sys,json; d=json.load(sys.stdin); g=d.get("permission_groups") or []; sys.exit(0 if any(x.get("id")=="net" and x.get("required") is True for x in g) else 1)' 2>/dev/null; then
    OK "$fqid@$VERSION carries the required permission group 'net'"
  else
    RED "$fqid@$VERSION does not carry a required 'net' group: ${body:0:300}"
    fail=1
  fi
done
[[ $fail -eq 0 ]] && OK "the closure is published; run 19-consent-install.py" || RED "the fixture is not what 19 needs"
exit $fail
