#!/bin/bash
# THE PUBLISHER-KEY GATE, against a real registry. The live half of registry 011/012.
#
#   MSYS_NO_PATHCONV=1 docker run --rm --network waffler_default \
#     -v "$PWD":/work -v "$PWD/sdk/cli/live_dev":/scripts \
#     -v syw-cli-cargo-target:/cargo-target -v syw-cli-cargo-home:/usr/local/cargo/registry \
#     -e CARGO_TARGET_DIR=/cargo-target \
#     rust:1 bash /scripts/25-publisher-rotation.sh
#
# ## What this proves that the unit tests cannot
#
# `publisher_rotation::judge` has nine unit tests and the orchestrator wiring has four. All of them
# run in-process against a sqlite memory pool. None of them proves that a REAL registry, serving over
# HTTP with its real migrations applied to a real database, refuses a key change — or that the anchor
# it refuses against survives the things a publisher can actually do to the catalog.
#
# ## The four legs, and why the third is the one that matters
#
#   1. a signed publish is accepted and ANCHORS the package's key
#   2. a second publish by the SAME key is accepted
#   3. a publish by a DIFFERENT key with no lineage is REFUSED, naming the lineage
#   4. WITHDRAWING the key-carrying versions does not reset the anchor
#
# Leg 4 is the regression guard for a bypass this gate shipped with. Withdrawal is authorised by
# `may_publish` over the tag -- an IdP token -- and NOT by the signing key, and `withdraw_version`
# DELETEs the catalog row. While the anchor was read from the newest key-carrying version row, a
# caller holding the token and not the key could withdraw every such version and publish their own
# key as a FIRST key under trust-on-first-use. Migration 012 moved the anchor into a row withdrawal
# cannot reach; this is the live proof.
#
# ## What is deliberately NOT here, and what that costs
#
# The ACCEPTED rotation -- a different key plus a lineage that verifies -- and both COMMITMENT cases
# with it. No shipping tool emits a lineage: `waffler key new` exists and `key rotate` does not,
# because the producer program was put to the owner and deferred ("not now; stay on the publishing
# cycle"). Crafting a lineage here would mean reimplementing the signature trailer in python, which
# would test the python.
#
# So of the registry's five verdicts, this file watches three where they run -- FirstKey (leg 1),
# Unchanged (leg 2), Refused (legs 3 and 4) -- and two only in-process: Rotated, and every refusal
# that turns on a commitment. That is the middle state of evidence, where a wrong CLOSED hides, and
# naming it is the point of writing it down.
#
# IT HAS ALREADY COST SOMETHING. Reading `next_key_digest = NULL` out of the live anchor table after
# a green run of this file is what led to the two commitment defects fixed in registry 9e6e405: the
# registry was re-deriving shared's commitment rules instead of calling them, and had both an
# immutability bypass and a lockout vector. Neither is reachable by any tool that exists, so neither
# could have been caught here -- and neither was caught by the nine unit tests either, because they
# asked what the verdict should be when the KEY changes and never what the COMMITMENT should be when
# it does not. What found them was reading a value this leg had produced and asking what it implied.
#
# ## What the live run measured, beyond pass/fail
#
# After a green run, the anchor row for the probe namespace reads:
#
#     publisher_key   = <the key from leg 1>
#     next_key_digest = NULL
#     package_versions rows for that namespace = 0
#
# Those three together are the actual proof of leg 4: the refusal quoted a key that NO surviving
# version row carried, so it can only have come from migration 012's table. The leg asserts the
# third line; the first two are read out of the database by hand.
#
# NULL is not a defect and does not foreclose rotation. `verify_rotation`'s `(None, declared)` arm
# lets a later same-key publish establish the commitment, so the level 1 -> 2 climb stays open and
# no recovery is needed. What is blocked today is emitting a commitment at all, which is the
# deferred producer -- so every package published now sits at level 1, able to climb the moment a
# tool can say anything.
set -uo pipefail

RED() { printf '\033[31m%s\033[0m\n' "$*"; }
OK() { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"
TAG="rot$(date +%s | tail -c 6)"
FQID="$TAG.probe"
PROJECT="/tmp/$FQID"
KEY_A="/tmp/key_a.hex"
KEY_B="/tmp/key_b.hex"
failures=0
fail() { RED "$*"; failures=$((failures + 1)); }

STEP "0. build the CLI"
cd /work/sdk/cli || exit 1
[[ -x "$WAFFLER" ]] || cargo build --quiet || { RED "the CLI did not build"; exit 1; }
"$WAFFLER" --version

STEP "1. an identity that owns its own tag"
# A FRESH TAG PER RUN. The anchor is per package and monotonic by design, so a run that reused a tag
# would be judging against the previous run's key and its first leg would refuse. That is the gate
# working, and it would read as a broken test.
TOKEN=$(curl -s --max-time 10 "$IDP/token?username=$TAG&entitlements=waffler-developer" \
  | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')
[[ -n "$TOKEN" ]] || { RED "no token minted"; exit 1; }
export WAFFLER_REGISTRY_TOKEN="$TOKEN"
OK "minted for $TAG (subject dev-$TAG), which will claim the tag on first publish"

STEP "2. two publisher keys"
rm -f "$KEY_A" "$KEY_B"
"$WAFFLER" key new "$KEY_A" >/dev/null || { RED "key A not created"; exit 1; }
"$WAFFLER" key new "$KEY_B" >/dev/null || { RED "key B not created"; exit 1; }
# The keys are hex seeds; the fingerprints below are what the registry's refusals will quote.
python3 - "$KEY_A" "$KEY_B" <<'PY'
import sys, hashlib
for label, path in zip("AB", sys.argv[1:3]):
    seed = bytes.fromhex(open(path).read().strip())
    print(f"  key {label}: seed {len(seed)} bytes, sha256 {hashlib.sha256(seed).hexdigest()[:16]}")
PY
OK "two distinct publisher keys"

STEP "3. scaffold and claim"
rm -rf "$PROJECT"
"$WAFFLER" scaffold "$FQID" --path "$PROJECT" --version "1.0.0" --sdk-path /work >/dev/null \
  || { RED "scaffold failed"; exit 1; }
cd "$PROJECT" || exit 1
# THE TAG IS CLAIMED OVER HTTP, not by a CLI command — there is none. The same call leg 01 makes.
CLAIM=$(curl -s --max-time 10 -X POST "$REGISTRY/v1/developer/namespaces" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"tag\":\"$TAG\"}" -w '\nhttp=%{http_code}\n')
echo "$CLAIM" | tail -2
echo "$CLAIM" | grep -qE "http=(200|201)" || { RED "the tag was not claimed; every publish below would be a 403"; exit 1; }
OK "$TAG claimed"

publish_as() { # $1 = version, $2 = key file
  local v="$1" k="$2"
  python3 - "$PROJECT/waffler.json" "$v" <<'PY'
import json, sys
p, v = sys.argv[1], sys.argv[2]
m = json.load(open(p))
m["version"] = v
json.dump(m, open(p, "w"), indent=2)
PY
  "$WAFFLER" publish --publisher-key "$k" --registry "$REGISTRY" 2>&1
}

# THE EXIT CODE DECIDES, NOT A GREP, and the first version of this leg got it wrong in the most
# instructive way available: it matched the word "published" to mean success, and that word appears in
# the REFUSAL -- "the last PUBLISHED version of this package was signed by ...". So a correct refusal
# read as an acceptance and the leg reported the working gate as two failures.
#
# Third time today a substring match on an error message produced a wrong verdict. Twice it turned a
# failure into a pass, which is the dangerous direction; here it turned a pass into a failure, which
# is merely embarrassing. The fix is the same either way: judge on the exit status, and use the
# message only to check WHAT the refusal names.
STEP "4. LEG 1 - a signed publish is accepted and anchors the key"
OUT=$(publish_as "1.0.0" "$KEY_A"); CODE=$?; echo "$OUT" | tail -3
[[ $CODE -eq 0 ]] || fail "the first signed publish was refused: $(echo "$OUT" | tail -2)"

STEP "5. LEG 2 - the SAME key publishes again"
OUT=$(publish_as "1.1.0" "$KEY_A"); CODE=$?; echo "$OUT" | tail -3
[[ $CODE -eq 0 ]] || fail "a republish by the anchored key was refused: $(echo "$OUT" | tail -2)"

STEP "6. LEG 3 - a DIFFERENT key with no lineage is REFUSED"
OUT=$(publish_as "2.0.0" "$KEY_B"); CODE=$?; echo "$OUT" | tail -4
if [[ $CODE -eq 0 ]]; then
  fail "A CHANGED PUBLISHER KEY WAS ACCEPTED. Every node would refuse this bundle after downloading it"
elif echo "$OUT" | grep -qi "lineage"; then
  OK "refused, and the refusal names the lineage a rotation must carry"
else
  fail "refused for a reason that does not mention the lineage: $(echo "$OUT" | tail -2)"
fi

catalog_code() { # $1 = version -> the HTTP status of its catalog row
  curl -s -o /dev/null -w '%{http_code}' --max-time 10 "$REGISTRY/v1/packages/$FQID/versions/$1"
}

STEP "6b. the withdrawal probe must be able to say BOTH things"
# The premise check in leg 4 reads a 404 as "withdrawn". A 404 is also what this endpoint returns
# for a URL shape it does not route, for a namespace that does not exist, and for a typo -- so a
# probe that is broken in any of those ways reports every version as withdrawn and leg 4's premise
# passes without a single row having been deleted. The probe has to be shown saying 200 about a
# version that IS there before its 404 about a version that is not can be believed.
#
# It also confirms legs 1 and 2 put rows in the CATALOG and not merely in the publish response.
both=1
for v in 1.0.0 1.1.0; do
  code=$(catalog_code "$v")
  if [[ "$code" == "200" ]]; then
    echo "  $v -> $code (present)"
  else
    echo "  $v -> $code"
    both=0
  fi
done
if [[ $both -eq 1 ]]; then
  OK "the probe reports 200 for a published version, so its 404 below carries information"
else
  fail "THE WITHDRAWAL PROBE IS BLIND: it does not report 200 for a version that was just published, so its 404 in leg 4 would mean nothing. Leg 4's premise cannot be trusted until this reads 200"
fi

STEP "7. LEG 4 - withdrawing the key-carrying versions must NOT reset the anchor"
# The bypass this gate shipped with. Withdrawal needs the token, not the key.
"$WAFFLER" unpublish "$FQID" 1.0.0 --registry "$REGISTRY" >/dev/null 2>&1 \
  || RED "  (1.0.0 withdrawal did not report success; the premise check below is the assertion)"
"$WAFFLER" unpublish "$FQID" 1.1.0 --registry "$REGISTRY" >/dev/null 2>&1 \
  || RED "  (1.1.0 withdrawal did not report success; the premise check below is the assertion)"
# ASSERT THE PREMISE, because this leg is worthless without it. Leg 4 differs from leg 3 in exactly
# ONE respect: no row carrying a publisher key remains. If the withdrawals did not actually delete
# those rows, leg 4 is leg 3 run a second time -- it would still be refused, still print its OK, and
# prove nothing whatever about the bypass it exists to guard. A leg whose distinguishing STARTING
# STATE is unestablished is indistinguishable from a leg that passes for the right reason.
#
# The exit status of `unpublish` is not enough. That reports what the CLI believes happened, and the
# thing the anchor read goes to is the catalog. So ask the catalog: a withdrawn version is a 404 on
# the version endpoint.
gone=1
for v in 1.0.0 1.1.0; do
  code=$(catalog_code "$v")
  if [[ "$code" == "404" ]]; then
    echo "  $v -> $code (withdrawn, so it carries no key the anchor read could find)"
  else
    echo "  $v -> $code"
    gone=0
  fi
done
if [[ $gone -ne 1 ]]; then
  fail "LEG 4 IS VACUOUS: a key-carrying version is still in the catalog, so this leg is not testing what happens when none is. Fix the withdrawal half before reading anything into the refusal below"
fi

OUT=$(publish_as "2.0.1" "$KEY_B"); CODE=$?; echo "$OUT" | tail -4
if [[ $CODE -eq 0 ]]; then
  fail "THE BYPASS IS OPEN: after withdrawing every key-carrying version, a different key published \
as a first key. Anyone holding a publish token can take over a package they cannot sign for"
else
  OK "still refused - the anchor survived the withdrawal of every version that carried it"
fi

printf '\n%s\n' "======================================================================"
if [[ $failures -gt 0 ]]; then
  RED "$failures FAILURE(S)"
  exit 1
fi
OK "THE KEY GATE HOLDS LIVE: anchored on first signed publish, same key passes, changed key refused,"
OK "and the anchor is not removable by withdrawal."
