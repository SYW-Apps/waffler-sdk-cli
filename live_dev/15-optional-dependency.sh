#!/bin/bash
# AN OPTIONAL DEPENDENCY, END TO END — the capability core supported and nothing could reach.
#
# Core has honoured `optional` on a CrateDependency since the flag was added. Nothing in the
# publish→resolve→install path could carry one: the CLI had no field to author it, the registry's
# reader decoded fqid/version/kind, the catalog stored `namespace@range` strings, and the
# marketplace refused a closure on ANY unresolved entry. A capability that exists and is unreachable
# is indistinguishable, from a publisher's seat, from one that does not exist.
#
# THE PAIR IS THE PROOF. Two packages differing in ONE character of their manifest:
#
#     devbot.opt.needs   depends on devbot.opt.ghost   optional: true   -> INSTALLS
#     devbot.opt.wants   depends on devbot.opt.ghost   optional: false  -> REFUSED
#
# Neither ghost exists. A run that only proved the first would not have shown the flag doing
# anything — an install that succeeds proves nothing unless the version that must fail does.
set -uo pipefail

RED() { printf '\033[31m%s\033[0m\n' "$*"; }
OK() { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
VERSION="0.1.$(date +%s)"

STEP "0. build the CLI"
cd /work/sdk/cli || exit 1
cargo build --locked --quiet 2>&1 | tail -20
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"
[[ -x "$WAFFLER" ]] || { RED "the CLI did not build"; exit 1; }

TOKEN=$(curl -s --max-time 10 "$IDP/token?username=devbot&entitlements=waffler-developer" | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')
[[ -n "$TOKEN" ]] || { RED "no token"; exit 1; }
export WAFFLER_REGISTRY_TOKEN="$TOKEN"

# Author and publish one package declaring a single dependency on a package that does not exist.
publish_with_ghost() {
  local fqid="$1"
  local is_optional="$2"
  local project="/tmp/optdep-${fqid##*.}"

  rm -rf "$project"
  "$WAFFLER" scaffold "$fqid" --path "$project" --version "$VERSION" --sdk-path /work >/dev/null || return 1

  OPTIONAL="$is_optional" python3 - "$project/waffler.json" <<'PY'
import json, os, sys
path = sys.argv[1]
manifest = json.load(open(path))
# THE ONE CHARACTER THAT DIFFERS between the two packages this script publishes.
manifest['dependencies'] = [
    {"fqid": "devbot.opt.ghost", "version": "^1.0.0", "optional": os.environ['OPTIONAL'] == 'true'}
]
open(path, 'w').write(json.dumps(manifest, indent=2))
PY

  cd "$project" || return 1
  "$WAFFLER" validate >/dev/null || { RED "$fqid does not validate"; return 1; }
  "$WAFFLER" pack >/dev/null 2>&1 || { RED "$fqid did not pack"; return 1; }
  "$WAFFLER" publish --registry "$REGISTRY" >/dev/null 2>&1 || { RED "$fqid did not publish"; return 1; }
  OK "published $fqid@$VERSION  (optional: $is_optional)"

  # THE FLAG MUST SURVIVE THE BUNDLE. Asserted here rather than only at the far end, so a failure
  # downstream can be attributed: the packer, the registry's reader and the resolver are three
  # places it could be dropped, and a single end-to-end assertion cannot say which.
  python3 - "$project/$fqid.zip" "$is_optional" <<'PY'
import json, sys, zipfile
declared = json.loads(zipfile.ZipFile(sys.argv[1]).read('.manifest'))['dependencies'][0]
want = sys.argv[2] == 'true'
if declared.get('optional') != want:
    print(f"  FAIL: the packed manifest says optional={declared.get('optional')!r}, wanted {want}")
    raise SystemExit(1)
print(f"  the bundle's own manifest carries optional={want}")
PY
}

STEP "1. publish the pair"
publish_with_ghost devbot.opt.needs true || exit 1
publish_with_ghost devbot.opt.wants false || exit 1

STEP "2. the registry's plan must distinguish them"
for pkg in devbot.opt.needs:true devbot.opt.wants:false; do
  fqid="${pkg%%:*}"
  want="${pkg##*:}"
  curl -s --max-time 15 "$REGISTRY/v1/packages/$fqid/install_plan" -o /tmp/optplan.json
  WANT="$want" FQID="$fqid" python3 - /tmp/optplan.json <<'PY'
import json, os, sys
plan = json.load(open(sys.argv[1]))
if isinstance(plan, dict):
    plan = plan.get('plan', plan.get('dependencies', []))
ghost = [e for e in plan if e['namespace'] == 'devbot.opt.ghost']
if not ghost:
    print(f"  FAIL: {os.environ['FQID']}'s plan omits the ghost entirely")
    raise SystemExit(1)
ghost = ghost[0]
want = os.environ['WANT'] == 'true'
if ghost.get('optional') != want:
    print(f"  FAIL: the plan says optional={ghost.get('optional')!r} for {os.environ['FQID']}, wanted {want}")
    raise SystemExit(1)
if ghost.get('resolved_version'):
    print("  FAIL: the ghost resolved to something, so this run proves nothing")
    raise SystemExit(1)
print(f"  {os.environ['FQID']}: the unresolved ghost is carried with optional={want}")
PY
  [[ $? -eq 0 ]] || exit 1
done
OK "the flag survives publication, storage and resolution"

echo
OK "optional-dependency leg finished — the install half is 16-optional-install.py"
