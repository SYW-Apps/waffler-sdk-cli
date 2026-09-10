# The full cycle, against a live node and a live registry

Nothing here is a mock. Every step runs against the beta node's own bridge (`/api/ws`) and a real
registry, and every assertion is about what those two actually did.

## What it proves, end to end

```
scaffold ─▶ validate ─▶ build ─▶ pack ─▶ publish ─▶ browse ─▶ install ─▶ ANSWER
                                            │                      │
                                            ▼                      ▼
                                        withdraw              uninstall
                                            │                      │
                                            ▼                      ▼
                                        republish  ────────▶  reinstall
```

Measured on 2026-09-10 against `waffler-beta` and `waffler-registry` on the `waffler_default`
network, with a package that did not exist an hour earlier:

| leg | what was proven |
|---|---|
| `01-pack-publish.sh` | a scaffolded project packs to a current-format bundle; publishing under a tag **somebody else holds is refused (403)**; claiming a tag then publishing succeeds; the registry stores the payload **byte-identical** and appends **exactly 64 signature bytes** |
| `02-install.py` | a second registry is added **alongside** the default; the node reaches it; search, detail, plan and preview all resolve; **install succeeds** |
| `03-grant-and-call.py` | with no rule the call is refused `AccessDenied`; a permission group + binding changes that to `NotFound` — the grant is what moved it |
| `04-verify-serves.py` | after a node restart the package **answers**, identifies itself, returns the payload it was handed, and **names** a capability it does not serve rather than echoing it |
| `05-uninstall.py` | uninstall **stops it serving** (`the package actor has been dropped`) and removes the record — asserted against a **working baseline**, so the transition is what is proven rather than the state |
| `06-withdraw-republish.sh` | republishing an existing version is refused (409); withdrawal makes the download 404 and the namespace **not browsable**; republish restores it; **two packs of one source are byte-identical**; a rebuild from source matches what the registry serves |
| `07-reinstall.py` | the loop closes on a genuinely different artifact — the republished bundle has a different content address |
| `09-dual-sign.sh` / `10-verify-chain.py` | the CLI signs as publisher, the registry **countersigns**, and an independent decoder confirms the chain: Publisher then Registry, the registry signature covering `payload \|\| publisher_signature` and **not** the payload alone. The node then installs it and **pins the publisher**, durably across a restart |
| `08-login.sh` | the interactive login, against a real provider: PKCE S256, a state parameter, an ephemeral loopback port; the credential is persisted **per registry** and does **not** leak to another; and a publish succeeds on the browser-obtained credential with **no environment token set** |

## Running it

The build legs run in `rust:1` on the compose network, because **a bundle is platform-specific**: the
toolchain emits `libfoo.so` on Linux and `foo.dll` on Windows, and the module a node loads is one of
those. Packing for a Linux node means packing on Linux. It is also the CI story.

```bash
# from the repository root
docker run --rm --network waffler_default \
  -v "$PWD":/work -v "$PWD/sdk/cli/live_dev":/scripts \
  -v syw-cli-cargo-target:/cargo-target -v syw-cli-cargo-home:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/cargo-target \
  rust:1 bash /scripts/01-pack-publish.sh

docker run --rm --network waffler_default -v "$PWD/sdk/cli/live_dev":/scripts \
  python:3.12-slim sh -c 'pip -q install msgpack websockets && python /scripts/02-install.py'
```

The node legs go through `/api/ws` — the seam the browser uses, carrying JSON-RPC 2.0 as MessagePack.
**Driving anything else would prove the bus works and say nothing about whether an operator can do
this.** Every call is one a person clicking in the marketplace makes.

Requests to the marketplace are **positional** MessagePack (its portal is explicit about this per
capability) and replies are named maps. That asymmetry is honoured rather than worked around, and each
positional shape is spelled out where it is built — a wire read by index is one where a field inserted
upstream silently shifts everything after it.

## Two things the cycle found that are NOT this tool's to fix

**1. A package installed from a marketplace cannot be called from the UI until someone grants it.**
The caller is `syw.app.web`, and the outbound Bus targets in its bundle were fixed when it was packed.
A package installed afterwards is not among them, so the call is refused — correctly, fail-closed —
with a message naming a fingerprint. Nothing surfaces this at install time: the install reports
success and the package is unreachable. `03-grant-and-call.py` shows the only route an operator has
today. Whether an install should offer to author that grant is core's call.

**2. Installed is not running.** A freshly installed package answers `NotFound: no handler registered`
until the node restarts; after the restart it serves. That is why `04-verify-serves.py` is a separate
script — a single process asserting across that discontinuity would be asserting across something it
cannot see.

## One assertion that was wrong, and why it is worth recording

An earlier version of the uninstall leg asserted "it must stop answering" and passed — while the
package had never answered in that process, because it was installed and not yet running. The
assertion was true and proved nothing: the observable was right and the reason was not. `05-uninstall.py`
now refuses to continue unless the baseline call succeeds first.
