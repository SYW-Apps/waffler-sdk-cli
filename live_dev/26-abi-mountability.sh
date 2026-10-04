#!/bin/bash
# CAN THIS HOST MOUNT WHAT THE REGISTRY SERVES AS NEWEST?
#
#   MSYS_NO_PATHCONV=1 docker run --rm --network waffler_default \
#     -v "$PWD":/work -v "$PWD/sdk/cli/live_dev":/scripts \
#     rust:1 bash /scripts/26-abi-mountability.sh
#
# ## The gap this exists because of
#
# Nothing in a bundle's manifest carries the guest ABI major. `core_compatibility` records the
# install-time check against core's CONTRACT GENERATION, which is a different number. So
# `install_plan` cannot filter on the ABI, a resolver hands a node the highest version its range
# admits, and if that artifact predates an ABI bump the ONLY evidence is an enable-time refusal.
#
# Confirmed with core 2026-10-04 and logged as ledger B8 (field + resolver filter, owner's call,
# recommended after the beta). **This leg is NOT that work.** It changes no resolution and adds no
# field. It reads what is already published and says whether the current host could mount it, which
# is the half that needs no design decision.
#
# ## Why it asks the artifact instead of the catalog
#
# The first version of this compared `published_at` against the ABI 2 -> 3 commit date and reported
# TWELVE traps. Every one was false, and behaviour refuted it: legs 19 and 22 passed the same day
# against an ABI-3 host with two of those "traps" installed and ENABLED. The commit date is when the
# change was AUTHORED; those artifacts were built in a container against a working tree that already
# carried it.
#
# So the date is wrong in BOTH directions -- old sources rebuilt late read as safe, new sources built
# early read as unsafe -- and a proxy for a missing field is not a measurement of it. The sound
# question is the one the loader asks: does the artifact EXPORT what the host refuses to load
# without.
#
# ## What a trap is, and is not
#
# A trap is an artifact that is BOTH unmountable AND the newest version of its package, because that
# is what a `*` range resolves to. An old version buried under a current one is reachable only by
# someone pinning it -- still broken, not yet in anybody's path. This leg fails on the first kind and
# reports the second as a count.
#
# ## IF THIS READS 0, THAT IS NOT A PROPERTY OF THE SYSTEM
#
# It read 0 of 20 on 2026-10-04 **because every fixture had just been republished from a current
# build** -- including `syw.probe.echo@1.0.4` and the clearing of a stray `0.2` line that was
# outranking every fresh publish. It is not that this cannot happen here: buried pre-bump versions
# that genuinely fail were confirmed the same day (`devbot.cnf.app@0.1.1789037165`,
# `devbot.dia.app@0.1.1790126630`), and they are reachable the moment anyone PINS one -- which is
# what a publisher does when bisecting a regression.
#
# So a 0 here means "nothing is in the resolution path today", never "the registry cannot serve an
# unmountable artifact". After the next ABI bump this goes red until the fixtures are republished,
# and that is the leg working.
set -uo pipefail

RED() { printf '\033[31m%s\033[0m\n' "$*"; }
OK() { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
HOST_SRC="/work/waffler_core/services/packages/src/process_host.rs"
failures=0
fail() { RED "$*"; failures=$((failures + 1)); }

command -v unzip >/dev/null 2>&1 || { apt-get -qq update >/dev/null 2>&1; apt-get -qq install -y unzip >/dev/null 2>&1; }
command -v nm >/dev/null 2>&1 || { RED "nm is not available; this leg needs binutils"; exit 1; }

STEP "0. the symbols the HOST refuses to load without"
# NAMED HERE, CHECKED AGAINST THE AUTHORITY. `wf_free` and `wf_shutdown` are resolved with
# `if let Ok` and are genuinely optional, so a guard built on "what the SDK macro emits" would demand
# them and get itself relaxed. The pair below is what `process_host` treats as a hard error.
#
# Hard-coded rather than parsed, because parsing Rust to find a symbol lookup is a worse bet than
# one grep that fails loudly when the set moves.
REQUIRED=(wf_abi_version wf_panic_strategy)
if [[ -f "$HOST_SRC" ]]; then
  for sym in "${REQUIRED[@]}"; do
    grep -q "$sym" "$HOST_SRC" \
      || fail "$sym is not mentioned in process_host.rs any more, so this leg's required set is stale"
  done
  OK "required set agrees with process_host.rs: ${REQUIRED[*]}"
else
  # NOT a silent pass. Without the repo this leg still works, but its premise is unverified and must
  # say so rather than look identical to a checked run.
  RED "  (process_host.rs not readable at $HOST_SRC - mount the repo with -v \"\$PWD\":/work to"
  RED "   verify the required set; continuing with it UNVERIFIED: ${REQUIRED[*]})"
fi

check_artifact() { # $1 = fqid, $2 = version -> 0 mountable, 1 missing a symbol, 2 unreadable
  local ns="$1" ver="$2" code so
  rm -rf /tmp/mnt && mkdir -p /tmp/mnt/x
  code=$(curl -s -o /tmp/mnt/b.zip -w '%{http_code}' --max-time 90 \
    "$REGISTRY/v1/packages/$ns/versions/$ver/download")
  [[ "$code" == "200" ]] || return 2
  # The bundle is <zip> || 64-byte signature, so unzip warns about the trailer. Judge on whether
  # files appeared, never on its exit code.
  unzip -qq -o /tmp/mnt/b.zip -d /tmp/mnt/x >/dev/null 2>&1
  so=$(find /tmp/mnt/x -name '*.so' -o -name '*.dll' 2>/dev/null | head -1)
  [[ -n "$so" ]] || return 3
  for sym in "${REQUIRED[@]}"; do
    nm -D --defined-only "$so" 2>/dev/null | grep -q " $sym\$" || { MISSING_SYM="$sym"; return 1; }
  done
  return 0
}

STEP "1. every package's NEWEST version"
PKGS=$(curl -s --max-time 20 "$REGISTRY/v1/packages" \
  | python3 -c 'import sys,json; [print(p["namespace"]) for p in json.load(sys.stdin).get("packages") or []]')
[[ -n "$PKGS" ]] || { RED "the registry listed no packages, so any verdict here is vacuous"; exit 1; }

examined=0; mountable=0; native=0
# ASSIGNED, NOT JUST DECLARED. `declare -a TRAPS` leaves the array UNSET under `set -u`, so
# `${#TRAPS[@]}` aborted the expansion -- and the effect was that step 1 counted 20 mountable
# artifacts and then printed NO VERDICT at all, neither pass nor fail. A silent skip in the step
# whose whole job is to report a count: exactly what this leg was written to prevent, in the leg
# itself. Found by reading the run rather than its exit code, which was 0.
TRAPS=()
for ns in $PKGS; do
  ver=$(curl -s --max-time 20 "$REGISTRY/v1/packages/$ns" | python3 -c 'import sys,json
d=json.load(sys.stdin); vs=d.get("versions") or []
print(max(vs, key=lambda v: v.get("published_at") or "")["version"] if vs else "")' 2>/dev/null)
  [[ -n "$ver" ]] || continue
  examined=$((examined + 1))
  MISSING_SYM=""
  check_artifact "$ns" "$ver"; rc=$?
  case $rc in
    0) native=$((native + 1)); mountable=$((mountable + 1));
       printf '  %-40s %-18s mountable\n' "$ns" "$ver" ;;
    1) native=$((native + 1)); TRAPS+=("$ns@$ver (no $MISSING_SYM)")
       printf '  %-40s %-18s MISSING %s\n' "$ns" "$ver" "$MISSING_SYM" ;;
    2) printf '  %-40s %-18s could not download\n' "$ns" "$ver" ;;
    3) printf '  %-40s %-18s no native artifact\n' "$ns" "$ver" ;;
  esac
done

echo
echo "  examined $examined newest version(s); $native carry a native artifact; $mountable mountable"
[[ $examined -gt 0 ]] || { RED "examined 0 artifacts, so the verdict below would be vacuous"; exit 1; }

if [[ ${#TRAPS[@]} -gt 0 ]]; then
  fail "${#TRAPS[@]} package(s) serve a NEWEST version this host cannot mount. A '*' range resolves to each of these, so an install succeeds and the enable is refused:"
  for t in "${TRAPS[@]}"; do RED "    $t"; done
else
  OK "no package's newest version is missing a symbol the host requires"
fi

STEP "2. THE DETECTOR MUST BE ABLE TO SAY THE OTHER THING"
# Without this, "0 traps" is indistinguishable from a check that cannot see. A pre-bump version
# buried under a current one is the natural negative control: still published, no longer newest, and
# genuinely unmountable. Two were confirmed by hand on 2026-10-04
# (devbot.dia.app@0.1.1789037165 and @0.1.1790126630, both missing wf_panic_strategy).
#
# It looks for ANY buried version that fails, rather than naming one: a named fixture gets withdrawn
# by another leg and then this step silently stops proving anything.
proved=0
for ns in $PKGS; do
  mapfile -t older < <(curl -s --max-time 20 "$REGISTRY/v1/packages/$ns" | python3 -c 'import sys,json
d=json.load(sys.stdin); vs=d.get("versions") or []
vs.sort(key=lambda v: v.get("published_at") or "")
[print(v["version"]) for v in vs[:-1]][:4]' 2>/dev/null)
  for ver in "${older[@]:0:3}"; do
    [[ -n "$ver" ]] || continue
    MISSING_SYM=""
    check_artifact "$ns" "$ver"; rc=$?
    if [[ $rc -eq 1 ]]; then
      OK "detector proven on a buried version: $ns@$ver is missing $MISSING_SYM"
      proved=1; break
    fi
  done
  [[ $proved -eq 1 ]] && break
done
if [[ $proved -eq 0 ]]; then
  # Not a failure of the registry. It means every published version is ABI-current, which is the
  # good state -- but it leaves step 1's pass unproven, and that has to be said rather than assumed.
  RED "  could not find any buried version missing a required symbol, so step 1's pass is"
  RED "  UNPROVEN this run: the detector was never shown reporting a trap."
fi

printf '\n%s\n' "======================================================================"
if [[ $failures -gt 0 ]]; then
  RED "$failures FAILURE(S)"
  exit 1
fi
OK "every newest artifact the registry serves can be mounted by this host"
