#!/bin/bash
# A ROOT NAMESPACE SHAPED LIKE A BUS FINGERPRINT, refused at the registry's door.
#
# A node refuses to mint a package identity for an fqid of exactly 32 lowercase hex characters: that
# is the shape of a bus FINGERPRINT, so a permission rule, a binding or a middleware scope naming
# such an fqid cannot be told apart from one naming a package's non-forgeable bus identity. Core
# closed that at install. The registry is the earlier door — refused here, a publisher learns it
# while claiming the name rather than when an install fails on somebody else's machine.
#
# WHAT THIS PROVES, and the third one is the point:
#   1. claiming a 32-lowercase-hex root is REFUSED, and the refusal says why and what to do instead;
#   2. the NEAR MISSES still claim — 31 hex, 33 hex, and 32 characters that are not all hex — because
#      a rule wider than the node's would be a rule nothing downstream enforces;
#   3. a fingerprint-shaped SEGMENT under a real root still PUBLISHES (`devbot.<32 hex>`), because
#      only the WHOLE identifier is ambiguous. This is the assertion that protects honest publishers,
#      and it is the one a careless tightening of rule 1 would break.
#
# NOT COVERED HERE, deliberately: the publish door's refusal of an fqid that is WHOLLY 32 hex. It is
# unreachable through this flow — publishing requires owning the root, and rule 1 refuses the claim —
# so reaching it needs an administrator to have granted the name first. The unit tests cover it
# (`an_fqid_that_is_WHOLLY_a_fingerprint_is_refused_as_malformed`); a live leg that pretended to
# exercise it would be asserting its own setup.
set -uo pipefail

RED()  { printf '\033[31m%s\033[0m\n' "$*"; }
OK()   { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
TAG="devbot"
FINGERPRINT="abcdef0123456789abcdef0123456789"
fail=0

# The near misses, each a fresh name per run so a previous run's claim cannot make this one pass by
# already owning it. All lowercase hex except where the point is that one character is not.
STAMP=$(printf '%030x' "$(date +%s)")
NEAR_31="a${STAMP:0:30}"          # 31 characters: one short
NEAR_33="a${STAMP:0:30}bb"        # 33 characters: one long
NEAR_NOTHEX="a${STAMP:0:30}g"     # 32 characters, and 'g' is not hex

STEP "0. mint a developer token"
TOKEN=$(curl -s --max-time 10 "$IDP/token?username=devbot&entitlements=waffler-developer" | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')
[[ -n "$TOKEN" ]] || { RED "no token"; exit 1; }
export WAFFLER_REGISTRY_TOKEN="$TOKEN"

token_for() {
  curl -s --max-time 10 "$IDP/token?username=$1&entitlements=waffler-developer" \
    | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])'
}

# EVERY CLAIM GETS ITS OWN DEVELOPER, and that is not tidiness. `MAX_TAGS_PER_DEVELOPER` is 1 on this
# registry, so a developer's SECOND claim is refused for QUOTA — a 403 whose message is about limits,
# which is indistinguishable from a shape refusal to anything reading a status code. The first run of
# this leg shared one developer and reported all three near misses as refused, when the fingerprint
# rule had never judged them at all: the quota answered first.
claim_as() {
  local tok
  tok=$(token_for "$1")
  [[ -n "$tok" ]] || { RED "no token for $1"; return 1; }
  curl -s --max-time 15 -o /tmp/claim-body.json -w '%{http_code}' \
    -X POST "$REGISTRY/v1/developer/namespaces" \
    -H "Authorization: Bearer $tok" -H 'Content-Type: application/json' \
    -d "{\"tag\":\"$2\"}"
}

STEP "1. a 32-lowercase-hex root is REFUSED, and the refusal is actionable"
# A developer holding no tags, so a 403 here can only be about the name.
code=$(claim_as "fpclaim$(date +%s)" "$FINGERPRINT")
body=$(cat /tmp/claim-body.json)
if [[ "$code" == "403" ]]; then
  OK "claiming '$FINGERPRINT' is refused (403)"
else
  RED "claiming '$FINGERPRINT' answered $code, not 403: ${body:0:200}"; fail=1
fi
# THE REASON REACHES THE CLAIMANT. A refusal nobody can learn from produces a second attempt that
# fails the same way, so the registry surfaces the rule's own text rather than a generic "reserved".
for phrase in "fingerprint" "Choose a root name"; do
  if printf '%s' "$body" | grep -qi -- "$phrase"; then
    OK "and it says '$phrase'"
  else
    RED "the refusal does not mention '$phrase': ${body:0:300}"; fail=1
  fi
done
# And it must NOT route the claimant at a tag request: an administrator granting this name would hand
# over a root whose own name can never be published.
if printf '%s' "$body" | grep -qi "file a tag request"; then
  RED "it points at a tag request, which is a dead end for this shape: ${body:0:300}"; fail=1
else
  OK "and does not send them to file a tag request for it"
fi

STEP "2. the NEAR MISSES still claim, because the shape is exact"
i=0
for near in "$NEAR_31" "$NEAR_33" "$NEAR_NOTHEX"; do
  i=$((i + 1))
  code=$(claim_as "fpnear$i$(date +%s)" "$near")
  body=$(cat /tmp/claim-body.json)
  if [[ "$code" == "201" ]]; then
    OK "'${near:0:12}…' (${#near} chars) claimed"
  else
    RED "'${near}' (${#near} chars) answered $code: ${body:0:200}"; fail=1
  fi
done

STEP "3. a fingerprint-shaped SEGMENT under a real root still publishes"
# THE OVER-REFUSAL GUARD, live. Only the whole identifier is ambiguous with a fingerprint; a node
# accepts `devbot.<32 hex>`, so the registry must publish it. If someone "tightens" the claim rule
# into the per-segment path, this is the leg that goes red.
curl -s --max-time 10 -X POST "$REGISTRY/v1/developer/namespaces" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"tag\":\"$TAG\"}" -o /dev/null   # idempotent: already claimed by the other legs

cd /work/sdk/cli || exit 1
cargo build --locked --quiet 2>&1 | tail -20
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"
[[ -x "$WAFFLER" ]] || { RED "the CLI did not build"; exit 1; }

FQID="$TAG.$FINGERPRINT"
VERSION="0.1.$(date +%s)"
PROJECT="/tmp/fp-segment-${VERSION//./_}"
rm -rf "$PROJECT"
if "$WAFFLER" scaffold "$FQID" --path "$PROJECT" --version "$VERSION" --sdk-path /work >/dev/null 2>&1; then
  OK "the CLI scaffolds '$FQID' — it does not refuse a hex segment either"
else
  RED "the CLI refused to scaffold '$FQID', which is a legal name"; fail=1
fi

if [[ -d "$PROJECT" ]]; then
  cd "$PROJECT" || exit 1
  "$WAFFLER" validate >/dev/null 2>&1 || { RED "'$FQID' does not validate"; "$WAFFLER" validate; fail=1; }
  "$WAFFLER" pack >/dev/null 2>&1 || { RED "'$FQID' did not pack"; fail=1; }
  if "$WAFFLER" publish --registry "$REGISTRY" >/dev/null 2>&1; then
    OK "and the registry PUBLISHED it"
  else
    RED "the registry refused '$FQID@$VERSION', which no node would refuse"; fail=1
  fi
  # Asserted at the catalog, not from the publish's exit code: a publish that reported success while
  # listing nothing is the failure this check exists for.
  if curl -s --max-time 10 "$REGISTRY/v1/packages/$FQID/versions/$VERSION" | grep -q "\"version\""; then
    OK "and the catalog serves $FQID@$VERSION"
  else
    RED "the catalog does not serve $FQID@$VERSION"; fail=1
  fi
fi

echo
[[ $fail -eq 0 ]] && OK "the fingerprint shape is refused as a ROOT and legal as a SEGMENT" \
                  || RED "the fingerprint rule is not what it should be"
exit $fail
