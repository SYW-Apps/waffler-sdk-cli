#!/bin/bash
# Publisher signing, end to end: the CLI signs, the registry countersigns, a node verifies.
#
# ## WHY A FRESH FQID EVERY RUN
#
# A publisher pin is PERMANENT and rotation is refused. A node that has installed a package pins its
# publisher forever, so re-running this against a fixed fqid with a newly generated key would be
# refused on the second run — correctly — and an hour would go into deciding whether that was a bug.
# The fqid carries a run id for exactly that reason.
set -uo pipefail

RED() { printf '\033[31m  FAIL  %s\033[0m\n' "$*"; FAILED=1; }
OK() { printf '\033[32m  OK    %s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }
FAILED=0

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
RUN="${RUN_ID:-$(date +%s)}"
FQID="devbot.signed.p$RUN"
VERSION="0.1.0"
PROJECT="/tmp/signed-$RUN"
KEY="/tmp/publisher-$RUN.key"

cd /work/sdk/cli || exit 1
cargo build --locked --quiet 2>&1 | tail -5
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"
export WAFFLER_REGISTRY_TOKEN=$(curl -s --max-time 10 "$IDP/token?username=devbot&entitlements=waffler-developer" | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')

STEP "1. create a publisher key, and read what it commits you to"
"$WAFFLER" key new "$KEY" || { RED "key new"; exit 1; }
PUBKEY=$("$WAFFLER" key new "$KEY" 2>&1 | head -1)
# The SECOND creation must be refused: replacing a publisher key destroys the only copy of something
# that cannot be reissued.
if "$WAFFLER" key new "$KEY" >/dev/null 2>&1; then RED "a publisher key was REPLACED"; else OK "a second creation is refused"; fi

STEP "2. an unsigned pack is still the ordinary path"
rm -rf "$PROJECT"
"$WAFFLER" scaffold "$FQID" --path "$PROJECT" --version "$VERSION" --sdk-path /work >/dev/null || exit 1
cd "$PROJECT" || exit 1
"$WAFFLER" pack 2>&1 | tail -1
# A registry-only bundle must stay completely legal — the key is opt-in, and making it mandatory
# would push every existing package through a door that cannot be reopened.
"$WAFFLER" pack 2>&1 | grep -q 'unsigned' && OK "packs unsigned without a key" || RED "an unsigned pack no longer reports unsigned"

STEP "3. pack WITH the publisher key"
"$WAFFLER" pack --publisher-key "$KEY" 2>&1 | tail -1
"$WAFFLER" pack --publisher-key "$KEY" 2>&1 | grep -q 'signed (trailer)' && OK "packs as a signature trailer" || RED "a signed pack does not report a trailer"

STEP "4. publish it — the registry must COUNTERSIGN rather than refuse"
"$WAFFLER" publish --registry "$REGISTRY" --no-build --publisher-key "$KEY" 2>&1 | tail -6
[[ ${PIPESTATUS[0]} -eq 0 ]] && OK "published" || { RED "the registry refused a publisher-signed upload"; exit 1; }

STEP "5. hand the artifact to the verifier"
curl -s --max-time 60 -o "/tmp/signed-$RUN.zip" "$REGISTRY/v1/packages/$FQID/versions/$VERSION/download"
echo "  downloaded $(stat -c%s "/tmp/signed-$RUN.zip") bytes"
echo "$FQID" > /tmp/dual-fqid.txt
echo "$PUBKEY" > /tmp/dual-pubkey.txt

echo
[[ $FAILED -eq 0 ]] && printf '\033[32mthe producer leg passed — verify the chain next\033[0m\n' || { printf '\033[31mthe producer leg FAILED\033[0m\n'; exit 1; }
