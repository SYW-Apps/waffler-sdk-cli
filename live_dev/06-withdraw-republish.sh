#!/bin/bash
# Withdraw, republish, and prove a bundle is a function of its inputs.
#
# `unpublish` exists because the CYCLE needs it. Publish, install, uninstall, withdraw, republish is
# the loop a developer actually runs while getting a package right, and a tool that can only add
# versions makes every mistake permanent — which pushes people to bump the version to escape a bad
# publish, and a version number meaning "the last one was wrong" means nothing.
set -uo pipefail

RED() { printf '\033[31m  FAIL  %s\033[0m\n' "$*"; FAILED=1; }
OK() { printf '\033[32m  OK    %s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }
FAILED=0

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
TAG="devbot"
FQID="$TAG.example.hello"
VERSION="0.1.0"
PROJECT="/tmp/hello"

cd /work/sdk/cli || exit 1
cargo build --locked --quiet 2>&1 | tail -5
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"
export WAFFLER_REGISTRY_TOKEN=$(curl -s --max-time 10 "$IDP/token?username=devbot&entitlements=waffler-developer" | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')

STEP "0. rebuild the project from scratch"
rm -rf "$PROJECT"
"$WAFFLER" scaffold "$FQID" --path "$PROJECT" --version "$VERSION" --sdk-path /work >/dev/null || { RED "scaffold"; exit 1; }
cd "$PROJECT" || exit 1
"$WAFFLER" pack 2>&1 | tail -2
BUNDLE="$PROJECT/$FQID.zip"
cp "$BUNDLE" /tmp/pack-1.zip

STEP "1. REPUBLISHING AN EXISTING VERSION MUST BE REFUSED"
# A registry that silently replaced a published version would let anyone who can publish swap the
# bytes under a version number every node has already resolved and cached.
"$WAFFLER" publish --registry "$REGISTRY" --no-build 2>&1 | tail -2
if [[ ${PIPESTATUS[0]} -eq 0 ]]; then RED "an already-published version was accepted again"; else OK "refused"; fi

STEP "2. it is served right now"
code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 30 "$REGISTRY/v1/packages/$FQID/versions/$VERSION/download")
[[ "$code" == "200" ]] && OK "download answers 200" || RED "download answered $code BEFORE withdrawal, so nothing below proves anything"

STEP "3. withdraw it"
"$WAFFLER" unpublish "$FQID" "$VERSION" --registry "$REGISTRY" || RED "unpublish failed"

STEP "4. the registry must stop serving that version"
code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 30 "$REGISTRY/v1/packages/$FQID/versions/$VERSION/download")
[[ "$code" == "200" ]] && RED "the download STILL answers 200 after withdrawal" || OK "download now answers $code"

STEP "5. and the namespace must stop being browsable, having no versions left"
curl -s --max-time 15 "$REGISTRY/v1/packages" | python3 -c '
import json,sys
names = [p.get("namespace") for p in json.load(sys.stdin).get("packages", [])]
listed = "devbot.example.hello" in names
print("  listed:", listed)
raise SystemExit(1 if listed else 0)
' && OK "not browsable" || RED "a namespace with no installable version is still browsable"

STEP "6. republish, so the cycle closes"
"$WAFFLER" publish --registry "$REGISTRY" --no-build 2>&1 | tail -2
[[ ${PIPESTATUS[0]} -eq 0 ]] && OK "republished" || RED "republish failed"

STEP "7. REPRODUCIBILITY — pack the same source again and compare bytes"
# A bundle must be a function of its inputs and nothing else. The first version of the writer stamped
# the build instant into the identity segment and left the archive's entry timestamps at their
# default; both are the clock, and both made a rebuild unverifiable against what was published.
rm -f "$BUNDLE"
sleep 2
"$WAFFLER" pack --no-build >/dev/null 2>&1
cmp -s /tmp/pack-1.zip "$BUNDLE" && OK "two packs of one source are byte-identical" || RED "the two packs differ"

STEP "8. and what the registry serves is the payload that was uploaded, plus a signature"
curl -s --max-time 60 -o /tmp/downloaded.zip "$REGISTRY/v1/packages/$FQID/versions/$VERSION/download"
python3 - <<'PY'
import os, sys
l = open('/tmp/pack-1.zip','rb').read()
b = open('/tmp/downloaded.zip','rb').read()
print(f"  local {len(l)} bytes; served {len(b)} bytes; appended {len(b)-len(l)}")
# OPAQUE CUSTODY: the bytes a node verifies must be the bytes that were signed, so anything that
# re-zipped or normalised the upload would produce an artifact core refuses as IntegrityFailure on a
# stranger's machine.
if b[:len(l)] != l:
    print("  PAYLOAD DIFFERS"); sys.exit(1)
if len(b) - len(l) != 64:
    print(f"  unexpected suffix length {len(b)-len(l)}"); sys.exit(1)
print("  payload identical, 64-byte signature appended")
PY
[[ $? -eq 0 ]] && OK "a rebuild from source matches what the registry serves" || RED "the served artifact does not match a rebuild"

echo
[[ $FAILED -eq 0 ]] && printf '\033[32mwithdraw/republish leg passed\033[0m\n' || { printf '\033[31mleg FAILED\033[0m\n'; exit 1; }
