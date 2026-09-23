#!/bin/bash
# The interactive login, end to end, against a real OpenID provider.
#
# ## WHY THIS EXISTS
#
# Everything in the publish legs used `WAFFLER_REGISTRY_TOKEN`, which is the CI path. The
# authorization-code flow — PKCE, the state check, the loopback listener, the code exchange, the
# persisted credential — had never run against anything: the development provider advertised only an
# issuer and a key set, so the flow could not be started and the whole lane was unexercised. A
# capability nothing can produce input for is a capability nobody has run and everybody assumes works.
#
# ## HOW A BROWSERLESS RUN DRIVES A BROWSER FLOW
#
# The CLI opens a browser AND prints the authorize URL, because a headless or remote shell has no
# browser and a URL on stdout is the only way through. So the harness plays the browser: it reads the
# URL the CLI printed, fetches it, and follows the 302 to the loopback listener the CLI is holding
# open. Nothing is simulated — the provider mints a real code against a real PKCE challenge, and the
# CLI exchanges it.
set -uo pipefail

RED() { printf '\033[31m  FAIL  %s\033[0m\n' "$*"; FAILED=1; }
OK() { printf '\033[32m  OK    %s\033[0m\n' "$*"; }
STEP() { printf '\n\033[36m==== %s ====\033[0m\n' "$*"; }
FAILED=0

REGISTRY="http://registry:42070"
FQID="devbot.example.hello"

cd /work/sdk/cli || exit 1
cargo build --locked --quiet 2>&1 | tail -5
WAFFLER="$CARGO_TARGET_DIR/debug/waffler"

# NO ENVIRONMENT TOKEN ANYWHERE IN THIS SCRIPT. If one were set, every assertion below would pass
# without the login lane running at all — the bearer would come from the environment and the stored
# credential would never be read.
unset WAFFLER_REGISTRY_TOKEN

STEP "0. start from no session"
"$WAFFLER" logout --registry "$REGISTRY" >/dev/null 2>&1
"$WAFFLER" whoami --registry "$REGISTRY" | tee /tmp/whoami-before.txt
grep -q 'not signed in' /tmp/whoami-before.txt && OK "no session" || RED "a session already exists, so the login below proves nothing"

STEP "1. run the login, and play the browser"
rm -f /tmp/login.log
"$WAFFLER" login --registry "$REGISTRY" >/tmp/login.log 2>&1 &
LOGIN_PID=$!

# Wait for the authorize URL to appear. The listener is bound before it is printed, so the URL's
# arrival means the loopback port is already accepting.
AUTH_URL=""
for _ in $(seq 1 60); do
  AUTH_URL=$(grep -o 'http://registry-idp:9099/authorize?[^ ]*' /tmp/login.log | head -1)
  [[ -n "$AUTH_URL" ]] && break
  sleep 0.5
done
if [[ -z "$AUTH_URL" ]]; then
  RED "the CLI printed no authorize URL"
  cat /tmp/login.log
  kill $LOGIN_PID 2>/dev/null
  exit 1
fi
echo "  authorize URL: ${AUTH_URL:0:110}..."

# THE PARAMETERS THE FLOW'S SECURITY RESTS ON, asserted on the URL the CLI actually built.
grep -q 'code_challenge_method=S256' <<<"$AUTH_URL" && OK "PKCE S256 challenge sent" || RED "no PKCE challenge — a code on a loopback redirect is a code any local process could race for"
grep -q 'state=' <<<"$AUTH_URL" && OK "state sent" || RED "no state parameter"
grep -q 'redirect_uri=http%3A%2F%2F127.0.0.1%3A' <<<"$AUTH_URL" && OK "loopback redirect, on an ephemeral port" || RED "unexpected redirect_uri"

# Follow it. The provider 302s to the loopback listener this process's child is holding.
curl -s -L -o /dev/null --max-time 30 "$AUTH_URL&username=devbot&entitlements=waffler-developer"

wait $LOGIN_PID
LOGIN_RC=$?
cat /tmp/login.log
[[ $LOGIN_RC -eq 0 ]] && OK "login completed" || RED "login exited $LOGIN_RC"

STEP "2. the identity is persisted, per registry"
"$WAFFLER" whoami --registry "$REGISTRY" | tee /tmp/whoami-after.txt
grep -q 'devbot' /tmp/whoami-after.txt && OK "signed in as devbot" || RED "whoami does not report the identity"
grep -q "$REGISTRY" /tmp/whoami-after.txt && OK "and names the registry it is for" || RED "whoami does not name the registry"

STEP "3. a DIFFERENT registry must still have no session"
# Credentials are per registry: signing in to one says nothing about another. A single cached token
# would let a `use` change carry an identity across a trust boundary silently.
"$WAFFLER" whoami --registry "http://registry-idp:9099" | tee /tmp/whoami-other.txt
grep -q 'not signed in' /tmp/whoami-other.txt && OK "the session did not leak to another registry" || RED "a credential for one registry answered for another"

STEP "4. publish using the STORED credential and no environment token"
cd /tmp/hello 2>/dev/null || { RED "no project; run 01-pack-publish.sh first"; exit 1; }
python3 - <<'PY'
import json, pathlib, re
p = '/tmp/hello/waffler.json'
d = json.load(open(p))
# A version the registry does not hold: the point is to exercise a real publish, and a republish of an
# existing version is refused before the credential is even used.
d['version'] = '0.1.1'
json.dump(d, open(p, 'w'), indent=2)

# AND THE VERSION THE CODE REPORTS ABOUT ITSELF, which the scaffold baked in as a constant. Bumping
# the manifest alone publishes a bundle whose artifact answers with the OLD version forever, and
# `04-verify-serves.py` compares exactly those two numbers to catch a stale module being served. A
# manifest-only bump therefore makes an honest node look like it is running a stale one - it did,
# and the false alarm was believed long enough to be written into a plan before the bundle was
# unzipped and found to contain no 0.1.1 string at all.
lib = pathlib.Path('/tmp/hello/src/lib.rs')
src = lib.read_text(encoding='utf-8')
bumped, n = re.subn(r'pub const VERSION: &str = "[^"]*";', 'pub const VERSION: &str = "0.1.1";', src)
assert n == 1, f'expected one VERSION constant in src/lib.rs, found {n}'
lib.write_text(bumped, encoding='utf-8')
PY
# WITHDRAW ANY 0.1.1 ALREADY IN THE CATALOG FIRST. A republish is refused, so a bundle published by
# an earlier run stays forever - and the earlier runs of this leg bumped the MANIFEST only, leaving
# an artifact that answers with the scaffold's baked 0.1.0. Legs that compare what a node records
# against what the artifact says about itself then report a stale module on a node that has none.
"$WAFFLER" unpublish "$FQID" 0.1.1 --registry "$REGISTRY" >/dev/null 2>&1 || true
"$WAFFLER" publish --registry "$REGISTRY" 2>&1 | tail -4
[[ ${PIPESTATUS[0]} -eq 0 ]] && OK "published with the browser-obtained credential" || RED "publish with the stored credential failed"

STEP "5. and it is in the catalog"
curl -s --max-time 15 "$REGISTRY/v1/packages/$FQID" | python3 -c '
import json,sys
versions = [v["version"] for v in json.load(sys.stdin).get("versions", [])]
print("  versions:", versions)
raise SystemExit(0 if "0.1.1" in versions else 1)
' && OK "0.1.1 is published" || RED "0.1.1 is not in the catalog"

echo
[[ $FAILED -eq 0 ]] && printf '\033[32mthe login lane passed\033[0m\n' || { printf '\033[31mthe login lane FAILED\033[0m\n'; exit 1; }
