#!/bin/bash
# THE DIAMOND. Every live install so far has been ONE package declaring no dependencies, so the
# registry's closure walk and the marketplace's "install straight down the list" have never once run
# against a graph. Both are unit-tested; neither has been asked a question a graph can answer.
#
# The shape that matters is not a chain — a chain works under any ordering that is even roughly
# right. It is the DIAMOND:
#
#        app ────────┐
#         │          │
#         ▼          ▼
#        lib ─────▶ util
#
# `util` is reachable at depth 1 (straight from app) and at depth 2 (through lib). A walk that
# records the depth of FIRST arrival gives it 1 — the same depth as `lib`, which needs it. Order by
# depth and the tie is broken by whatever order the frontier happened to be in.
#
# WHY A DIAMOND IS THE MINIMUM. Two packages prove nothing: any ordering rule puts a leaf before its
# only dependent. Three in a chain prove nothing either, because depths are then 0/1/2 and distinct.
# The diamond is the smallest graph where a node's shortest path and its longest path differ, which
# is exactly the case depth-ordering cannot see.
set -uo pipefail

RED() { printf '\033[31m%s\033[0m\n' "$*"; }
OK() { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
TAG="devbot"
# A FRESH PATCH EVERY RUN, because republishing a version is refused (409) and a leg that only works
# the first time is a leg nobody runs twice. It buys a second proof for free: after any run but the
# first, several versions satisfy `^0.1.0`, so the plan resolving to the one just published is the
# range picking the HIGHEST rather than the first it found. That is asserted below.
VERSION="0.1.$(date +%s)"

# The authored graph, written ONCE and used for two different jobs: to author the manifests, and to
# check the plan. It is the same text in both places deliberately — a check that re-declares the
# graph in its own words is a check that can agree with a mistake.
GRAPH='{
  "devbot.dia.app":  ["devbot.dia.lib", "devbot.dia.util"],
  "devbot.dia.lib":  ["devbot.dia.util"],
  "devbot.dia.util": []
}'

STEP "0. build the CLI"
cd /work/sdk/cli || exit 1
cargo build --locked --quiet 2>&1 | tail -20
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"
[[ -x "$WAFFLER" ]] || { RED "the CLI did not build"; exit 1; }

STEP "1. mint a developer token and claim the tag"
TOKEN=$(curl -s --max-time 10 "$IDP/token?username=devbot&entitlements=waffler-developer" | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')
[[ -n "$TOKEN" ]] || { RED "no token"; exit 1; }
export WAFFLER_REGISTRY_TOKEN="$TOKEN"
# Already held from an earlier leg; re-claiming is not an error worth stopping for, and the publish
# below is what actually proves the tag is ours.
curl -s --max-time 10 -X POST "$REGISTRY/v1/developer/namespaces" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"tag\":\"$TAG\"}" -o /dev/null -w 'claim http=%{http_code}\n'

# Author, pack and publish one package with a declared dependency list.
#
# PUBLISHED LEAF-FIRST, which is NOT the thing under test. The registry accepts a package whose
# dependencies are unpublished — it resolves at plan time, not at publish time — so publish order is
# free. It is done leaf-first only so a failure here is unambiguous about which package failed.
publish_one() {
  local fqid="$1"
  local deps="$2"
  # SEPARATE STATEMENTS, not one `local a= b= c=`. Bash expands every argument to the builtin before
  # running it, so a later assignment referring to an earlier one on the same line sees it unset —
  # which under `set -u` is a hard error rather than an empty string.
  local project="/tmp/dia-${fqid##*.}"

  rm -rf "$project"
  "$WAFFLER" scaffold "$fqid" --path "$project" --version "$VERSION" --sdk-path /work >/dev/null || return 1

  DEPS="$deps" python3 - "$project/waffler.json" <<'PY'
import json, os, sys
path = sys.argv[1]
manifest = json.load(open(path))
# `fqid@range` is what the registry stores and re-splits. A RANGE, never a pin: pinning here would
# make every upstream patch a republish of everything downstream.
manifest['dependencies'] = [
    {"fqid": f, "version": "^0.1.0"} for f in json.loads(os.environ['DEPS'])
]
open(path, 'w').write(json.dumps(manifest, indent=2))
PY

  cd "$project" || return 1
  "$WAFFLER" validate >/dev/null || { RED "$fqid does not validate"; return 1; }
  "$WAFFLER" pack >/dev/null 2>&1 || { RED "$fqid did not pack"; return 1; }
  "$WAFFLER" publish --registry "$REGISTRY" >/dev/null 2>&1 || { RED "$fqid did not publish"; return 1; }
  OK "published $fqid  deps=$deps"
}

STEP "2. publish the diamond"
publish_one devbot.dia.util '[]' || exit 1
publish_one devbot.dia.lib  '["devbot.dia.util"]' || exit 1
publish_one devbot.dia.app  '["devbot.dia.lib","devbot.dia.util"]' || exit 1

STEP "3. the closure the registry resolves for the root"
curl -s --max-time 15 "$REGISTRY/v1/packages/devbot.dia.app/install_plan" -o /tmp/plan.json \
  -w 'http=%{http_code}\n'
python3 -m json.tool < /tmp/plan.json

STEP "4. IS IT INSTALLABLE IN THE ORDER GIVEN?"
# The only property that matters, and it is not "sorted by depth": every package must appear AFTER
# everything it declares. That is a topological order, and depth is at best a proxy for one.
GRAPH="$GRAPH" VERSION="$VERSION" python3 - /tmp/plan.json <<'PY'
import json, os, sys

graph = json.loads(os.environ['GRAPH'])
expected_version = os.environ['VERSION']
plan = json.load(open(sys.argv[1]))
if isinstance(plan, dict):
    plan = plan.get('plan', plan.get('dependencies', []))

order = [e['namespace'] for e in plan]
print("plan order:", " -> ".join(order))
print("depths:    ", ", ".join(f"{e['namespace']}={e['depth']}" for e in plan))

missing = set(graph) - set(order)
if missing:
    print(f"\nFAIL: the closure omits {sorted(missing)}")
    raise SystemExit(1)

# A dependency's POSITION is the whole question. `index` is safe here only because the membership
# check above already passed.
violations = []
for package, declared in graph.items():
    for dependency in declared:
        if order.index(dependency) > order.index(package):
            violations.append(f"{package} is installed before {dependency}, which it declares")

for entry in plan:
    if entry.get('unresolved_reason'):
        violations.append(f"{entry['namespace']} did not resolve: {entry['unresolved_reason']}")
    # THE RANGE MUST PICK THE HIGHEST, not the first published that fits. Every run leaves another
    # version behind, so from the second run on this is a real question: `^0.1.0` is satisfied by
    # every one of them, and only the newest is the right answer.
    elif entry['resolved_version'] != expected_version:
        violations.append(
            f"{entry['namespace']} resolved to {entry['resolved_version']}, "
            f"not the {expected_version} published moments ago"
        )

if violations:
    print("\nFAIL: the plan is not installable in the order it gives")
    for v in violations:
        print("  -", v)
    # NAMED, because the interesting part is WHICH pair inverted. "not topological" sends a reader
    # back to re-derive the graph they already have.
    raise SystemExit(1)

print("\nOK: every package appears after everything it declares")
PY
ORDER_RC=$?

STEP "5. AN UNRESOLVABLE DEPENDENCY MUST BE IN THE PLAN, NOT ABSENT FROM IT"
# A plan that silently contained only what resolved would look complete: the caller installs most of
# what it needs and finds the rest missing at run time, on a node, in a message about something else.
publish_one devbot.dia.orphan '["devbot.dia.ghost"]' || exit 1
curl -s --max-time 15 "$REGISTRY/v1/packages/devbot.dia.orphan/install_plan" -o /tmp/orphan.json \
  -w 'http=%{http_code}\n'
python3 - /tmp/orphan.json <<'PY'
import json, sys
plan = json.load(open(sys.argv[1]))
if isinstance(plan, dict):
    plan = plan.get('plan', plan.get('dependencies', []))
print(json.dumps(plan, indent=2))

ghost = [e for e in plan if e['namespace'] == 'devbot.dia.ghost']
if not ghost:
    print("\nFAIL: the unresolvable dependency is missing from the plan entirely")
    raise SystemExit(1)
reason = ghost[0].get('unresolved_reason')
if not reason:
    print("\nFAIL: it is in the plan but carries no reason, so a caller cannot say why")
    raise SystemExit(1)
if ghost[0].get('resolved_version') or ghost[0].get('content_address'):
    print("\nFAIL: it is marked unresolved AND carries a version to fetch")
    raise SystemExit(1)
print(f"\nOK: present, unresolved, and it says why -- {reason}")
PY
ORPHAN_RC=$?

echo
if [[ $ORDER_RC -ne 0 || $ORPHAN_RC -ne 0 ]]; then
  RED "dependency-closure leg FAILED"
  exit 1
fi
OK "dependency-closure leg finished"
