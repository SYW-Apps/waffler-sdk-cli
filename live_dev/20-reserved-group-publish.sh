#!/bin/bash
# A BUNDLE FROM ANOTHER PACKER, declaring a permission group in the HOST's namespace.
#
# The node refuses to install a manifest declaring a group under `host.` — the host synthesizes groups
# there from operator decisions and approves them itself — and `waffler validate`/`pack` refuses to
# produce one, both on waffler_shared's `is_reserved_group_id`. The registry is the third door: a bundle
# that did not come through this CLI reaches it with nothing else having looked, and publishing it would
# list a version no node can ever install.
#
# So this leg MAKES such a bundle the way another packer would — an ordinary pack, with the group added
# to its manifest afterwards — and proves, in the order that makes each step mean something:
#   1. the CLI refuses the same declaration at validate (the pack-time door);
#   2. the registry refuses the doctored bundle, naming the reserved namespace, and does NOT list it;
#   3. the SAME bundle without that one group publishes at the SAME version — so the refusal was the
#      group, not the namespace, the version or the upload. Without this control, a registry refusing
#      everything would pass step 2.
set -uo pipefail

RED()  { printf '\033[31m%s\033[0m\n' "$*"; }
OK()   { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
TAG="devbot"
FQID="devbot.rsv.squat"
# A FRESH PATCH EVERY RUN: step 3 publishes this version, and republishing a version is refused (409).
VERSION="0.1.$(date +%s)"
failures=0
fail() { RED "  FAIL  $*"; failures=$((failures + 1)); }
pass() { OK "  OK  $*"; }

# One group in core's full schema. Only its id differs from an ordinary author's group, so the refusal
# can be attributed to the id alone.
export HOST_GROUP='{"id": "host.middleware_grant", "display_name": "squat", "description": "live-leg fixture: an author writing the host rule", "status": "PendingReview", "required": true, "source": "devbot.rsv.squat", "rules": [], "system": false, "default_bindings_locked": false}'

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

STEP "2. an ordinary package, packed by this CLI"
PROJECT="/tmp/rsv-${VERSION//./_}"
rm -rf "$PROJECT" "$PROJECT-host"
"$WAFFLER" scaffold "$FQID" --path "$PROJECT" --version "$VERSION" --sdk-path /work >/dev/null || { RED "scaffold failed"; exit 1; }
cd "$PROJECT" || exit 1
"$WAFFLER" pack --output /tmp/rsv-honest.zip >/dev/null 2>&1 || { RED "the honest package did not pack"; exit 1; }
pass "packed $FQID@$VERSION"

STEP "3. THE CLI REFUSES the same declaration (the pack-time door)"
cp -r "$PROJECT" "$PROJECT-host"
python3 - "$PROJECT-host/waffler.json" <<'PY'
import json, os, sys
path = sys.argv[1]
manifest = json.load(open(path))
manifest['permissionGroups'] = [json.loads(os.environ['HOST_GROUP'])]
open(path, 'w').write(json.dumps(manifest, indent=2))
PY
validate_out=$(cd "$PROJECT-host" && "$WAFFLER" validate 2>&1); validate_rc=$?
if [[ $validate_rc -ne 0 && "$validate_out" == *"reserved"* && "$validate_out" == *"host.middleware_grant"* ]]; then
  pass "waffler validate refused it, naming the reserved namespace"
else
  fail "waffler validate did not refuse the host group (exit $validate_rc): ${validate_out:0:400}"
fi

STEP "4. the same package, DOCTORED as another packer would"
# Every entry byte-identical except `.manifest`, which gains the group. Unsigned, so there is no signature
# for the rewrite to break — the registry signs what it accepts.
python3 - /tmp/rsv-honest.zip /tmp/rsv-doctored.zip <<'PY'
import json, os, sys, zipfile
src, dst = sys.argv[1], sys.argv[2]
group = json.loads(os.environ['HOST_GROUP'])
with zipfile.ZipFile(src) as zin, zipfile.ZipFile(dst, 'w') as zout:
    for info in zin.infolist():
        data = zin.read(info.filename)
        if info.filename == '.manifest':
            manifest = json.loads(data)
            manifest.setdefault('permission_groups', []).append(group)
            data = json.dumps(manifest).encode()
        zout.writestr(info, data)
# THE DOCTORING PROVES ITSELF before anything relies on it: the group is present, and nothing else moved.
with zipfile.ZipFile(src) as a, zipfile.ZipFile(dst) as b:
    assert sorted(a.namelist()) == sorted(b.namelist()), "entry list changed"
    for name in a.namelist():
        if name != '.manifest':
            assert a.read(name) == b.read(name), f"{name} changed"
    ids = [g.get('id') for g in json.loads(b.read('.manifest')).get('permission_groups', [])]
    assert 'host.middleware_grant' in ids, f"the group is not in the doctored manifest: {ids}"
print("  doctored: .manifest gains host.middleware_grant; every other entry byte-identical")
PY
[[ $? -eq 0 ]] || { RED "the doctoring did not apply — nothing below would mean anything"; exit 1; }

STEP "5. THE REGISTRY REFUSES the doctored bundle, and does not list it"
publish_out=$("$WAFFLER" publish --bundle /tmp/rsv-doctored.zip --registry "$REGISTRY" 2>&1); publish_rc=$?
if [[ $publish_rc -ne 0 && "$publish_out" == *"reserved"* && "$publish_out" == *"host.middleware_grant"* ]]; then
  pass "publish refused, naming the reserved namespace"
else
  fail "publish of the doctored bundle was not refused for the reserved group (exit $publish_rc): ${publish_out:0:500}"
fi
listed=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 "$REGISTRY/v1/packages/$FQID/versions/$VERSION")
if [[ "$listed" == "404" ]]; then
  pass "$FQID@$VERSION is not listed (http 404)"
else
  fail "$FQID@$VERSION answers http $listed after a refused publish"
fi

STEP "6. THE CONTROL: the honest bundle publishes at the same version"
honest_out=$("$WAFFLER" publish --bundle /tmp/rsv-honest.zip --registry "$REGISTRY" 2>&1); honest_rc=$?
if [[ $honest_rc -eq 0 ]]; then
  pass "the same package without the host group published"
else
  fail "the honest bundle was refused too, so step 5 proves nothing about the group (exit $honest_rc): ${honest_out:0:500}"
fi
listed=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 "$REGISTRY/v1/packages/$FQID/versions/$VERSION")
[[ "$listed" == "200" ]] && pass "$FQID@$VERSION is listed (http 200)" || fail "$FQID@$VERSION answers http $listed after its publish"

echo
if [[ $failures -eq 0 ]]; then OK "the reserved-group publish leg passed"; else RED "$failures failure(s)"; fi
exit $failures
