#!/bin/bash
# The developer cycle, run where a node actually runs: Linux.
#
# WHY IN A CONTAINER RATHER THAN ON THE HOST. A package bundle is platform-specific — the toolchain
# emits libfoo.so on Linux and foo.dll on Windows, and the module a node loads is one of those. So
# packing for the beta node means packing on Linux. This is also the CI story, which is the one that
# has to work.
set -uo pipefail

RED() { printf '\033[31m%s\033[0m\n' "$*"; }
# A COUNTER, because the two checks at the end of this leg had none and therefore were not checks.
# `set -uo pipefail` without `-e` means the script's exit status is the LAST command's, so a step
# that printed a wrong answer and then ran a successful `curl` reported success.
failures=0
fail() { RED "$*"; failures=$((failures + 1)); }
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
# THE EXIT CODE IS READ, not printed. `echo "publish exit=$?"` is what this was, which reports the
# number and acts on nothing -- and the number it reports is routinely non-zero, because these legs
# are built to be re-runnable and a version already in the catalog is refused.
#
# Three outcomes, and they are not two: UPLOADED, ALREADY THERE, and REFUSED FOR SOME OTHER REASON.
# Collapsing the last two into "non-zero is fine" is how a leg keeps running after the registry
# started rejecting every publish. Step 8 also needs to know WHICH of the first two happened, since
# the custody comparison can only mean something about an artifact this run actually uploaded.
PUBLISH_OUT=$("$WAFFLER" publish --registry "$REGISTRY" 2>&1); PUBLISH_CODE=$?
echo "$PUBLISH_OUT" | tail -3
UPLOADED_THIS_RUN=0
if [[ $PUBLISH_CODE -eq 0 ]]; then
  UPLOADED_THIS_RUN=1
  OK "this run uploaded $VERSION"
elif echo "$PUBLISH_OUT" | grep -q "is already published"; then
  # The whole phrase. An earlier leg in this suite matched the bare word "already" and hit "the
  # bundle is already signed", exiting 0 having published nothing.
  OK "$VERSION was already in the catalog from an earlier run, which is the expected re-run state"
else
  fail "the publish was refused for a reason that is not 'already published': $(echo "$PUBLISH_OUT" | tail -2)"
fi

STEP "7. is it in the catalog?"
curl -s --max-time 10 "$REGISTRY/v1/packages/$FQID" | python3 -m json.tool | head -40

STEP "8. download it: the registry is the signing authority, so what comes back must differ"
# WHAT THIS STEP USED TO DO: print `payload prefix identical: False` under a comment saying the
# payload MUST be byte-identical, and exit 0. It had no assertion at all -- three `print` calls --
# so the one observable contradicting the claim above it was emitted as information and passed.
#
# It also printed `appended suffix: -29568 bytes`, calling a NEGATIVE difference an appended suffix.
# A label that cannot be false about its own number is not a reading.
#
# AND THE COMPARISON WAS NOT ALWAYS THE RIGHT PAIR. On a re-run the publish is refused as already
# published, so the catalog still holds the FIRST run's artifact while the local bundle has just
# been rebuilt from source -- a different compilation, a different zip, a different size. The two
# were never going to match, and nothing in the step knew which case it was in. That is why it is
# split below: one assertion that holds on every run, and one that holds only on the run that
# actually uploaded.
#
# THE PAYLOAD HALF IS NOT LOST BY SKIPPING IT HERE. `06-withdraw-republish.sh` step 8 asserts it
# where the pair is guaranteed right -- that leg withdraws the version and republishes it first --
# and pins the appended suffix at exactly 64 bytes. This step was a second copy of that check,
# placed where it could not work, which is how it came to print a bare `False` for three weeks
# under a comment calling the property mandatory.
CONTENT_ADDRESS=$(curl -s --max-time 10 "$REGISTRY/v1/packages/$FQID/versions/$VERSION" \
  | python3 -c 'import sys,json; print(json.load(sys.stdin).get("content_address",""))' 2>/dev/null)
echo "catalog content_address: ${CONTENT_ADDRESS:-<none>}"
curl -s --max-time 60 -o /tmp/downloaded.zip -w 'http=%{http_code} bytes=%{size_download}\n' \
  "$REGISTRY/v1/packages/$FQID/versions/$VERSION/download"
python3 - "$BUNDLE" "$CONTENT_ADDRESS" "$UPLOADED_THIS_RUN" <<'PY'
import hashlib, os, sys

local, address, uploaded = sys.argv[1], sys.argv[2], sys.argv[3] == "1"
down = "/tmp/downloaded.zip"
if not os.path.exists(down) or os.path.getsize(down) < 1000:
    print("FAIL no usable download:", open(down).read()[:200] if os.path.exists(down) else "missing")
    raise SystemExit(1)

b = open(down, "rb").read()
l = open(local, "rb").read()
print(f"local bundle {len(l)} bytes; downloaded {len(b)} bytes; difference {len(b) - len(l)}")

bad = 0

# ------------------------------------------------------------------- holds on EVERY run
# The address is `integrity::address_of` over the bytes the registry took custody of, and the
# download is served by opening that same address. So this says: the registry serves the bytes whose
# hash it advertises. It needs nothing from the local build, which is what makes it the assertion
# that survives a re-run -- and it is the integrity contract a node actually depends on, since a
# node checks what it downloaded against what the catalog told it to expect.
got = "sha256:" + hashlib.sha256(b).hexdigest()
if not address:
    print("FAIL the catalog reported no content_address, so the download cannot be checked against it")
    bad += 1
elif got == address:
    print(f"  OK  the download hashes to the address the catalog advertises ({address[:23]}...)")
else:
    print(f"FAIL the download does NOT hash to the advertised address: catalog {address}, served {got}")
    print("     a node would refuse this as IntegrityFailure")
    bad += 1

# ------------------------------------------------- holds only on the run that actually uploaded
# OPAQUE CUSTODY of the payload: the bytes a node verifies must be the bytes that were signed, so
# anything that re-zipped or normalised the upload would produce an artifact core refuses as
# IntegrityFailure on a stranger's machine. Only checkable against a local bundle that IS the
# uploaded one.
if uploaded:
    if b[: len(l)] == l:
        print("  OK  the stored artifact begins with the exact bytes that were uploaded")
    else:
        print("FAIL the stored artifact does NOT begin with the uploaded payload -- the registry")
        print("     re-zipped or normalised it, and every node would refuse it as IntegrityFailure")
        bad += 1
    if len(b) > len(l):
        print(f"  OK  a {len(b) - len(l)}-byte trailer was appended, which is the countersignature")
    else:
        print(f"FAIL the stored artifact is not LARGER than the upload (difference {len(b) - len(l)}),")
        print("     so nothing was appended and the registry did not countersign it")
        bad += 1
else:
    # A SKIP THAT CANNOT BE MISTAKEN FOR A PASS. It names what went unchecked, why, and what makes
    # it checkable again -- because this branch is the one taken on every run after the first, and a
    # quiet skip there would retire the claim permanently while still printing green.
    print("SKIP the payload comparison, and it is not owed here. This run did not upload: the")
    print("     catalog holds an earlier run's artifact for this version while the local bundle was")
    print("     just rebuilt from source, so a mismatch would say nothing.")
    print("     `06-withdraw-republish.sh` step 8 makes this exact assertion and CAN: it withdraws")
    print("     the version and republishes it in step 6, so the bundle it compares is by")
    print("     construction the one that was uploaded -- and it pins the suffix at exactly 64")
    print("     bytes rather than merely positive. This step was a copy of that one, in the only")
    print("     place it could never hold. The address check above still ran and still holds.")

raise SystemExit(1 if bad else 0)
PY
[[ $? -eq 0 ]] || fail "the download does not satisfy the registry's own custody claim (step 8)"

STEP "9. install plan — what a marketplace would resolve"
curl -s --max-time 10 "$REGISTRY/v1/packages/$FQID/install_plan" | python3 -m json.tool | head -30

echo
printf '%s\n' "======================================================================"
if [[ $failures -gt 0 ]]; then
  RED "$failures FAILURE(S) in the pack-and-publish leg"
  exit 1
fi
OK "pack-and-publish leg finished"
