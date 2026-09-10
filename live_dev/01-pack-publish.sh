#!/bin/bash
# The developer cycle, run where a node actually runs: Linux.
#
# WHY IN A CONTAINER RATHER THAN ON THE HOST. A package bundle is platform-specific — the toolchain
# emits libfoo.so on Linux and foo.dll on Windows, and the module a node loads is one of those. So
# packing for the beta node means packing on Linux. This is also the CI story, which is the one that
# has to work.
set -uo pipefail

RED() { printf '\033[31m%s\033[0m\n' "$*"; }
OK() { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
# A tag this identity will CLAIM, rather than one somebody else already holds. The first run of this
# script published under `syw.` and the registry refused with 403 — correctly: the `syw` tag belongs
# to subject `dev-syw-dev`, and the token was minted for `devbot`. That refusal is the ownership gate
# working, and it is asserted below rather than worked around.
TAG="devbot"
FQID="$TAG.example.hello"
VERSION="0.1.0"
PROJECT="/tmp/hello"

STEP "0. build the CLI"
cd /work/sdk/cli || exit 1
cargo build --locked --quiet 2>&1 | tail -20
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"
[[ -x "$WAFFLER" ]] || { RED "the CLI did not build"; exit 1; }
"$WAFFLER" --version

STEP "1. scaffold a package the node has never seen"
rm -rf "$PROJECT"
"$WAFFLER" scaffold "$FQID" --path "$PROJECT" --version "$VERSION" --sdk-path /work || exit 1

STEP "2. validate before building — the cheap check"
cd "$PROJECT" || exit 1
"$WAFFLER" validate || exit 1

STEP "3. pack: build the crate and write the bundle"
"$WAFFLER" pack 2>&1 | tail -8
BUNDLE="$PROJECT/$FQID.zip"
[[ -f "$BUNDLE" ]] || { RED "no bundle was written"; exit 1; }
python3 - "$BUNDLE" <<'PY'
import zipfile, json, os, sys
p = sys.argv[1]
z = zipfile.ZipFile(p)
print("--- entries ---")
for n in z.namelist():
    i = z.getinfo(n)
    print(f"  {n}  ({i.file_size} bytes, stored={i.compress_type==0})")
m = json.loads(z.read('.manifest'))
print("--- .manifest ---")
print(json.dumps(m, indent=2, sort_keys=True))
print(f"--- file is {os.path.getsize(p)} bytes ---")
PY

STEP "4. mint a developer token"
TOKEN=$(curl -s --max-time 10 "$IDP/token?username=devbot&entitlements=waffler-developer" | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')
[[ -n "$TOKEN" ]] || { RED "no token"; exit 1; }
export WAFFLER_REGISTRY_TOKEN="$TOKEN"
OK "token minted for devbot"

STEP "5a. THE OWNERSHIP GATE: publishing under a tag somebody else holds must be refused"
# `syw` belongs to dev-syw-dev. This identity is devbot. A tool that could publish here would be able
# to impersonate another developer's namespace.
mkdir -p /tmp/squat && cp -r "$PROJECT"/* /tmp/squat/
python3 - <<'PY'
import json
p='/tmp/squat/waffler.json'
# The manifest is JSON with comment keys; load and rewrite just the fqid.
raw = open(p).read()
d = json.loads(raw)
d['fqid'] = 'syw.example.hello'
open(p,'w').write(json.dumps(d, indent=2))
PY
cd /tmp/squat && "$WAFFLER" publish --registry "$REGISTRY" 2>&1 | tail -3
SQUAT_RC=${PIPESTATUS[0]}
if [[ $SQUAT_RC -eq 0 ]]; then RED "SECURITY: a foreign namespace was accepted"; exit 1; fi
OK "refused, as it must be"
cd "$PROJECT" || exit 1

STEP "5b. claim the tag this identity will publish under"
curl -s --max-time 10 -X POST "$REGISTRY/v1/developer/namespaces" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"tag\":\"$TAG\"}" -w '\nhttp=%{http_code}\n' | tail -4

STEP "6. publish"
"$WAFFLER" publish --registry "$REGISTRY"
echo "publish exit=$?"

STEP "7. is it in the catalog?"
curl -s --max-time 10 "$REGISTRY/v1/packages/$FQID" | python3 -m json.tool | head -40

STEP "8. download it: the registry is the signing authority, so what comes back must differ"
curl -s --max-time 60 -o /tmp/downloaded.zip -w 'http=%{http_code} bytes=%{size_download}\n' \
  "$REGISTRY/v1/packages/$FQID/versions/$VERSION/download"
python3 - "$BUNDLE" <<'PY'
import os, sys
local, down = sys.argv[1], "/tmp/downloaded.zip"
if not os.path.exists(down) or os.path.getsize(down) < 1000:
    print("no usable download:", open(down).read()[:200] if os.path.exists(down) else "missing")
    raise SystemExit
b = open(down,'rb').read(); l = open(local,'rb').read()
print(f"uploaded {len(l)} bytes; downloaded {len(b)} bytes; difference {len(b)-len(l)}")
# THE PAYLOAD MUST BE BYTE-IDENTICAL. The registry holds it in OPAQUE CUSTODY: the bytes a node
# verifies must be the bytes that were signed, so anything that re-zipped or normalised the upload
# would produce an artifact core refuses as IntegrityFailure on a stranger's machine.
print("payload prefix identical:", b[:len(l)] == l)
print("appended suffix:", len(b)-len(l), "bytes")
PY

STEP "9. install plan — what a marketplace would resolve"
curl -s --max-time 10 "$REGISTRY/v1/packages/$FQID/install_plan" | python3 -m json.tool | head -30

echo
OK "pack-and-publish leg finished"
