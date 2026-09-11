#!/bin/bash
# WITHDRAW SOMETHING SOMEBODY DEPENDS ON. `06` withdraws a package nothing needs; this withdraws a
# DEPENDENCY and asks what its dependent looks like afterwards.
#
# The mechanism is structural — withdrawal deletes the version row, so `list_versions` returns
# nothing and the resolver has nothing to choose. That is exactly the kind of "guaranteed by
# construction" claim that has been wrong twice in this harness, so it is measured rather than
# assumed.
#
# THE QUESTION THIS LEG IS REALLY ASKING is not whether the plan breaks. It is what an operator sees
# BEFORE they try:
#
#     devbot.wd.lib   withdrawn      -> gone from browse, download 404s
#     devbot.wd.app   still published -> still listed, still looks installable, CANNOT be installed
#
# A listing that looks fine and cannot be installed is a gap only a resolve would reveal. If nothing
# in the catalog marks it, that is a finding rather than a failure — the registry cannot re-derive a
# dependent's health on every write without walking every package that ever named it.
set -uo pipefail

RED() { printf '\033[31m%s\033[0m\n' "$*"; }
OK() { printf '\033[32m%s\033[0m\n' "$*"; }
NOTE() { printf '\033[33m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
VERSION="0.1.$(date +%s)"
FAILED=0

STEP "0. build the CLI and mint a token"
cd /work/sdk/cli || exit 1
cargo build --locked --quiet 2>&1 | tail -20
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"
[[ -x "$WAFFLER" ]] || { RED "the CLI did not build"; exit 1; }
TOKEN=$(curl -s --max-time 10 "$IDP/token?username=devbot&entitlements=waffler-developer" | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')
[[ -n "$TOKEN" ]] || { RED "no token"; exit 1; }
export WAFFLER_REGISTRY_TOKEN="$TOKEN"

publish_one() {
  local fqid="$1"
  local deps="$2"
  local project="/tmp/wd-${fqid##*.}"
  rm -rf "$project"
  "$WAFFLER" scaffold "$fqid" --path "$project" --version "$VERSION" --sdk-path /work >/dev/null || return 1
  DEPS="$deps" python3 - "$project/waffler.json" <<'PY'
import json, os, sys
path = sys.argv[1]
manifest = json.load(open(path))
manifest['dependencies'] = [
    {"fqid": f, "version": "^0.1.0", "optional": False} for f in json.loads(os.environ['DEPS'])
]
open(path, 'w').write(json.dumps(manifest, indent=2))
PY
  cd "$project" || return 1
  "$WAFFLER" validate >/dev/null || return 1
  "$WAFFLER" pack >/dev/null 2>&1 || return 1
  "$WAFFLER" publish --registry "$REGISTRY" >/dev/null 2>&1 || return 1
  OK "published $fqid@$VERSION"
}

STEP "1. publish a dependent and the dependency it needs"
publish_one devbot.wd.lib '[]' || exit 1
publish_one devbot.wd.app '["devbot.wd.lib"]' || exit 1

STEP "2. the closure resolves BEFORE the withdrawal — or nothing below proves anything"
curl -s --max-time 15 "$REGISTRY/v1/packages/devbot.wd.app/install_plan" -o /tmp/wd-before.json
python3 - /tmp/wd-before.json <<'PY'
import json, sys
plan = json.load(open(sys.argv[1]))
if isinstance(plan, dict):
    plan = plan.get('plan', plan.get('dependencies', []))
lib = next((e for e in plan if e['namespace'] == 'devbot.wd.lib'), None)
if lib is None or not lib.get('resolved_version'):
    print("  FAIL: the closure did not resolve before the withdrawal")
    raise SystemExit(1)
print(f"  devbot.wd.lib resolves to {lib['resolved_version']}")
PY
[[ $? -eq 0 ]] || exit 1

STEP "3. withdraw the DEPENDENCY"
"$WAFFLER" unpublish devbot.wd.lib "$VERSION" --registry "$REGISTRY" || { RED "unpublish failed"; exit 1; }

code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 30 "$REGISTRY/v1/packages/devbot.wd.lib/versions/$VERSION/download")
[[ "$code" == "200" ]] && { RED "the withdrawn dependency still downloads"; FAILED=1; } || OK "its download answers $code"

STEP "4. THE DEPENDENT IS STILL PUBLISHED — what does the catalog say about it?"
curl -s --max-time 15 "$REGISTRY/v1/packages/devbot.wd.app" -o /tmp/wd-app.json
python3 - /tmp/wd-app.json <<'PY'
import json, sys
detail = json.load(open(sys.argv[1]))
versions = detail.get('versions', [])
if not versions:
    print("  the dependent is no longer listed — withdrawal cascaded, which would be a surprise")
    raise SystemExit(1)
print(f"  devbot.wd.app is still listed with {len(versions)} version(s)")
# Does ANY field mark it as having an unsatisfiable dependency?
text = json.dumps(detail)
marks = [k for k in ('unresolvable', 'broken', 'degraded', 'health') if k in text]
print(f"  fields hinting at its health: {marks or 'none'}")
PY

STEP "5. ...and its closure is now unresolvable, which is the only place it shows"
curl -s --max-time 15 "$REGISTRY/v1/packages/devbot.wd.app/install_plan" -o /tmp/wd-after.json
python3 - /tmp/wd-after.json <<'PY'
import json, sys
plan = json.load(open(sys.argv[1]))
if isinstance(plan, dict):
    plan = plan.get('plan', plan.get('dependencies', []))
lib = next((e for e in plan if e['namespace'] == 'devbot.wd.lib'), None)
if lib is None:
    print("  FAIL: the withdrawn dependency vanished from the plan instead of being reported")
    raise SystemExit(1)
if lib.get('resolved_version'):
    print(f"  FAIL: it still resolves to {lib['resolved_version']} after being withdrawn")
    raise SystemExit(1)
reason = lib.get('unresolved_reason') or ''
if not reason:
    print("  FAIL: unresolved with no reason, so a caller cannot say why")
    raise SystemExit(1)
print(f"  unresolved, and it says why -- {reason}")
PY
[[ $? -eq 0 ]] || FAILED=1

STEP "6. republish it and the closure must recover"
cd /tmp/wd-lib || exit 1
"$WAFFLER" publish --registry "$REGISTRY" >/dev/null 2>&1 || { RED "republish failed"; exit 1; }
curl -s --max-time 15 "$REGISTRY/v1/packages/devbot.wd.app/install_plan" -o /tmp/wd-again.json
python3 - /tmp/wd-again.json <<'PY'
import json, sys
plan = json.load(open(sys.argv[1]))
if isinstance(plan, dict):
    plan = plan.get('plan', plan.get('dependencies', []))
lib = next((e for e in plan if e['namespace'] == 'devbot.wd.lib'), None)
if not lib or not lib.get('resolved_version'):
    print("  FAIL: the closure did not recover after republication")
    raise SystemExit(1)
print(f"  resolves again to {lib['resolved_version']} — withdrawal is reversible")
PY
[[ $? -eq 0 ]] || FAILED=1

echo
[[ $FAILED -eq 0 ]] && OK "withdraw-a-dependency leg passed" || { RED "leg FAILED"; exit 1; }
