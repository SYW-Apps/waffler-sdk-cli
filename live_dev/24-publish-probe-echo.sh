#!/bin/bash
# Publish the BAKED version of syw.probe.echo to the registry. Idempotent; re-run after a reset.
#
#   MSYS_NO_PATHCONV=1 docker run --rm --network waffler_default \
#     -v "$PWD":/work -v "$PWD/sdk/cli/live_dev":/scripts \
#     -v syw-cli-cargo-target:/cargo-target -v syw-cli-cargo-home:/usr/local/cargo/registry \
#     -e CARGO_TARGET_DIR=/cargo-target \
#     rust:1 bash /scripts/24-publish-probe-echo.sh
#
# ## The decision this implements
#
# `docker/core-web/packages.manifest` declares `syw.probe.echo` at 1.0.3 and the registry topped out
# at 1.0.2, which waffler_ui pinned in two places. Two ends could have moved; the OWNER chose to move
# the REGISTRY. This leg is that choice, made re-runnable — a registry reset drops 1.0.3 and then
# waffler_ui's pins break again for a reason nobody would connect to the reset.
#
# ## Two artifacts, and why the baked zip is not the one to upload
#
# pack_bundle's own words: TWO DISTRIBUTION PATHS, TWO ARTIFACTS.
#
#   baked install     SIGNED with the pinned dev key, verified by core against its trust anchors.
#                     An unsigned bundle is refused at that gate.
#   registry publish  uploaded UNSIGNED. The registry is the signing authority on that path and
#                     countersigns what it accepts, so it REFUSES an artifact already carrying a
#                     signature — it cannot decide which trailing bytes are signature and which
#                     are content, and being wrong there produces a corrupt package that publishes
#                     successfully.
#
# So this packs its own unsigned artifact. The first version of this leg uploaded the baked zip,
# which failed exactly that way.
#
# ## What the two versions differ by, because it is not a number
#
# 1.0.2 declares no middleware. 1.0.3 declares `probe-intercept`, and that declaration omits BOTH
# `required` and `kind`, whose defaults are `true` and `Enforcing`. So the difference is not what a
# node must approve — it is whether the package can start at all without an operator in the loop.
#
# ## Identity
#
# The `syw` tag belongs to subject `dev-syw-dev`, and `01-pack-publish.sh` exists partly to prove
# `devbot` is REFUSED there. The dev IdP derives subject as `dev-<username>`, hence `syw-dev`. On a
# real registry this would be an authenticated human; here the IdP mints for anybody who asks, by
# design, and that is the only reason this script can run at all.
set -uo pipefail

RED() { printf '\033[31m%s\033[0m\n' "$*"; }
OK() { printf '\033[32m%s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }

REGISTRY="http://registry:42070"
IDP="http://registry-idp:9099"
FQID="syw.probe.echo"
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"

STEP "0. read the declaration from packages.manifest, the single source of truth"
# NOT from the baked zip and not from constants here. The manifest declares the version and the
# middleware; the baked zip is one derived artifact and the registry needs a different one.
LINE=$(grep -v '^#' /work/docker/core-web/packages.manifest | grep "^${FQID};" | head -1)
[[ -n "$LINE" ]] || { RED "$FQID is not declared in packages.manifest"; exit 1; }
IFS=';' read -r m_fqid m_version m_crate m_artifact m_bus m_prim m_lanes m_compat m_depends m_uiplug m_mw <<<"$LINE"
VERSION="$m_version"
echo "  $m_fqid $VERSION   crate=$m_crate   artifact=$m_artifact"
echo "  middleware: ${m_mw:-(none)}" | cut -c1-140

# REFUSE WHAT THIS LEG CANNOT EXPRESS. `pack-packages.sh` handles eleven columns; this leg handles the
# two $FQID actually uses. If anyone gives it bus targets, primitives, fast lanes, dependencies or a
# UI plugin, packing without them would publish a bundle that LOOKS right and declares LESS than the
# baked one — so the leg stops. A guard that silently drops a column it never read is how a registry
# ends up serving a quietly weaker package than the image does.
for pair in "bus_targets:$m_bus" "primitives:$m_prim" "fast_lanes:$m_lanes" "depends:$m_depends" "ui_plugin:$m_uiplug"; do
  name="${pair%%:*}"; val="${pair#*:}"
  if [[ -n "$val" ]]; then
    RED "packages.manifest now declares $name for $FQID ('$val')."
    RED "This leg packs only --core-compat and --middleware. Extend it, or use pack-packages.sh."
    RED "Do NOT publish a bundle that drops a declaration the baked one carries."
    exit 1
  fi
done
OK "every column this leg does not handle is empty"

STEP "1. build the artifact and the packer"
command -v cargo >/dev/null || { RED "no cargo in this image"; exit 1; }
cd /work || exit 1
cargo build --release --quiet --manifest-path "$m_crate" || { RED "the crate did not build"; exit 1; }
SO="${CARGO_TARGET_DIR:-/work/target}/release/$m_artifact"
[[ -f "$SO" ]] || { RED "no built artifact at $SO"; exit 1; }
echo "  artifact: $SO ($(stat -c%s "$SO") bytes)"
cargo build --release --quiet --manifest-path live_dev/sim_current_runtime/tools/pack_bundle/Cargo.toml \
  || { RED "pack_bundle did not build"; exit 1; }
PACKER="${CARGO_TARGET_DIR:-/work/target}/release/pack_bundle"
[[ -x "$PACKER" ]] || { RED "no pack_bundle at $PACKER"; exit 1; }

STEP "2. pack UNSIGNED — the registry's artifact, not the node's"
BUNDLE="/tmp/${FQID}-unsigned.zip"
rm -f "$BUNDLE"
PACK_ARGS=(--fqid "$m_fqid" --version "$VERSION" --artifact "$SO" --out "$BUNDLE" --unsigned)
[[ -n "$m_compat" ]] && PACK_ARGS+=(--core-compat "$m_compat")
[[ -n "$m_mw" ]] && PACK_ARGS+=(--middleware "$m_mw")
"$PACKER" "${PACK_ARGS[@]}" || { RED "pack failed"; exit 1; }
[[ -f "$BUNDLE" ]] || { RED "no unsigned bundle was written"; exit 1; }
# READ BACK WHAT LANDED rather than trusting the packer's exit code.
python3 /scripts/_read_bundle.py "$BUNDLE" || { RED "the packed bundle did not read back"; exit 1; }

STEP "3. build the CLI and mint the tag owner's token"
[[ -x "$WAFFLER" ]] || cargo build --quiet --manifest-path sdk/cli/Cargo.toml
[[ -x "$WAFFLER" ]] || { RED "no CLI at $WAFFLER"; exit 1; }
"$WAFFLER" --version
TOKEN=$(curl -s --max-time 10 "$IDP/token?username=syw-dev&entitlements=waffler-developer" \
  | python3 -c 'import sys,json; print(json.load(sys.stdin)["access_token"])')
[[ -n "$TOKEN" ]] || { RED "no token minted"; exit 1; }
export WAFFLER_REGISTRY_TOKEN="$TOKEN"
OK "token minted for syw-dev (subject dev-syw-dev)"

STEP "4. publish"
# IDEMPOTENT BY THE CATALOG'S OWN UNIQUENESS, not by a check here. The orchestrator detects a
# duplicate at `record` rather than earlier because "a check earlier is one two concurrent publishes
# both pass". So a refusal naming an existing version IS the desired end state.
OUT=$("$WAFFLER" publish --bundle "$BUNDLE" --registry "$REGISTRY" 2>&1)
CODE=$?
echo "$OUT" | tail -6
if [[ $CODE -eq 0 ]]; then
  OK "published $FQID $VERSION"
elif echo "$OUT" | grep -qF "is already published"; then
  # THE FULL PHRASE, from package_store's own message. The first version of this matched the WORD
  # "already" and reported success against "the bundle is already SIGNED" — a leg that exited 0
  # having published nothing. Same substring-on-an-error-message defect as classifying a refusal by
  # its code when several sites answer that code.
  OK "$FQID $VERSION was already published — nothing to do"
elif echo "$OUT" | grep -qF "is already signed"; then
  RED "the bundle handed to publish is SIGNED. This leg packs unsigned, so something changed."
  exit 2
else
  # DECLINED rather than guessed at. An unrecognised refusal funnelled into the nearest known bucket
  # is how a wrong remedy gets prescribed with confidence.
  RED "publish refused for a reason this leg does not recognise (exit $CODE). Not classified."
  exit 1
fi

STEP "5. verify through a consumer, not through the receipt"
echo "  a publish receipt is not a listing. Run, from the repository root:"
echo "    MSYS_NO_PATHCONV=1 docker run --rm --network waffler_default \\"
echo "      -v \"\$PWD/packages/marketplace/live_dev\":/s python:3.12-slim \\"
echo "      sh -c 'pip -q install msgpack websockets >/dev/null 2>&1 && python /s/00-node-preconditions.py'"
OK "publish leg finished"
