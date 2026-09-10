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

and, for a package that needs other packages:

           publish a graph ─▶ resolve the closure ─▶ install it in order ─▶ all ENABLED
```

Measured on 2026-09-10 against `waffler-beta` and `waffler-registry` on the `waffler_default`
network, with a package that did not exist an hour earlier:

| leg | what was proven |
|---|---|
| `01-pack-publish.sh` | a scaffolded project packs to a current-format bundle; publishing under a tag **somebody else holds is refused (403)**; claiming a tag then publishing succeeds; the registry stores the payload **byte-identical** and appends **exactly 64 signature bytes** |
| `02-install.py` | a second registry is added **alongside** the default; the node reaches it; search, detail, plan and preview all resolve; **install succeeds** |
| `03-grant-and-call.py` | with no rule the call is refused `AccessDenied`, **naming the caller as `syw.app.web`** rather than only its fingerprint; a permission group + binding changes that to `NotFound` — the grant is what moved it. Takes `CYCLE_FQID`, and REFUSES TO START rather than reporting failures when its preconditions are not met |
| `04-verify-serves.py` | after a node restart the package **answers**, identifies itself, returns the payload it was handed, and **names** a capability it does not serve rather than echoing it |
| `05-uninstall.py` | uninstall **stops it serving** (`the package actor has been dropped`) and removes the record — asserted against a **working baseline**, so the transition is what is proven rather than the state |
| `06-withdraw-republish.sh` | republishing an existing version is refused (409); withdrawal makes the download 404 and the namespace **not browsable**; republish restores it; **two packs of one source are byte-identical**; a rebuild from source matches what the registry serves |
| `07-reinstall.py` | the loop closes on a genuinely different artifact — the republished bundle has a different content address |
| `09-dual-sign.sh` / `10-verify-chain.py` | the CLI signs as publisher, the registry **countersigns**, and an independent decoder confirms the chain: Publisher then Registry, the registry signature covering `payload \|\| publisher_signature` and **not** the payload alone. The node then installs it and **pins the publisher**, durably across a restart |
| `08-login.sh` | the interactive login, against a real provider: PKCE S256, a state parameter, an ephemeral loopback port; the credential is persisted **per registry** and does **not** leak to another; and a publish succeeds on the browser-obtained credential with **no environment token set** |
| `11-dependency-closure.sh` | publishes a **diamond** and asks the registry to resolve it. The plan must be installable **in the order it gives**, every package after everything it declares; the range must pick the **highest** published, not the first that fits; a dependency that cannot resolve must be **an entry carrying a reason**, not an omission; and two packages asking for **incompatible ranges** of one dependency must be **refused, naming both asks**. **Two of those failed on first run** — see below |
| `12-install-closure.py` | installs that closure through the marketplace on the live node: all three land, **in order**, **enabled**, and **at the version the registry offered** — a stale row would otherwise read as success. Both kinds of unresolvable closure — a dependency that does not exist, and one no version satisfies — are **refused with nothing installed**, naming what disagrees; a repeat install is **not a second copy** |
| `13-closure-serves.py` | after a restart, **every member of the closure answers** — not just the root — identifies itself, and reports the version the node records. Then it takes the leaf away and reports what a single process can see of that |
| `14-gate-after-restart.py` | the observable neither 13 nor `04` can reach: **run twice across a node restart**, it proves all three serve, removes the leaf, and after the restart the dependents must **not have started**. It reads its phase from the NODE rather than a flag, so the two runs cannot be done out of order |

## Running it

The build legs run in `rust:1` on the compose network, because **a bundle is platform-specific**: the
toolchain emits `libfoo.so` on Linux and `foo.dll` on Windows, and the module a node loads is one of
those. Packing for a Linux node means packing on Linux. It is also the CI story.

```bash
# from the repository root. MSYS_NO_PATHCONV=1 is REQUIRED on Windows and harmless elsewhere — see below.
MSYS_NO_PATHCONV=1 docker run --rm --network waffler_default \
  -v "$PWD":/work -v "$PWD/sdk/cli/live_dev":/scripts \
  -v syw-cli-cargo-target:/cargo-target -v syw-cli-cargo-home:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/cargo-target \
  rust:1 bash /scripts/01-pack-publish.sh

MSYS_NO_PATHCONV=1 docker run --rm --network waffler_default -v "$PWD/sdk/cli/live_dev":/scripts \
  python:3.12-slim sh -c 'pip -q install msgpack websockets && python /scripts/02-install.py'
```

**Why the prefix.** Git Bash on Windows rewrites a bare `/unix/path` in argv into a Windows path
before docker ever sees it, so the command above without it produces:

```
bash: C:/Program Files/Git/scripts/01-pack-publish.sh: No such file or directory
```

Measured, not assumed — that is the documented command run verbatim on this machine, exit 127. It
fails loudly here, which is the good case. **The dangerous version is the same rewrite inside a probe
whose failure is swallowed**: `docker exec c grep -c X /usr/local/bin/prog 2>/dev/null` never looks at
the file and reports a confident `0`, indistinguishable from "the string is not there". A path that
travels inside a quoted `sh -c '...'` is left alone, which is why the second command needs no
escaping of its `/scripts/02-install.py`.

The `docker logs | grep` lines further down carry no path in argv and are unaffected.

The node legs go through `/api/ws` — the seam the browser uses, carrying JSON-RPC 2.0 as MessagePack.
**Driving anything else would prove the bus works and say nothing about whether an operator can do
this.** Every call is one a person clicking in the marketplace makes.

Requests to the marketplace are **positional** MessagePack (its portal is explicit about this per
capability) and replies are named maps. That asymmetry is honoured rather than worked around, and each
positional shape is spelled out where it is built — a wire read by index is one where a field inserted
upstream silently shifts everything after it.

## Three things the cycle found that were NOT this tool's to fix

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

**3. A required dependency left and the dependents kept serving.** ✅ **CLOSED** — fixed in core at
`677364a5`, deployed, and confirmed live by `14-gate-after-restart.py`. Originally: `packages:uninstall`
accepted removing `devbot.dia.util` while `devbot.dia.lib` and `devbot.dia.app` both declared it
REQUIRED, and neither dependent was removed or stopped, across a node restart.

Core's cause: `enable` had refused without required dependencies since it was written. `init` — the
other path that starts a package, and the one a restart takes — spawned every enabled record without
asking. **A gate on one of two entry points is not a gate.**

Confirmed on the deployed image: both dependents fail to start after a restart, and the boot log
carries one line each naming the dependent, the missing dependency and its range —

```
ERROR supervisor: package 'devbot.dia.lib' is enabled but 1 of its required dependencies is
  missing, so it was NOT started. It stays enabled and will start on the next boot once they
  are present: 'devbot.dia.util' (^0.1.0) is not installed
WARN  package error: ... code=DependencyUnmet
```

What did **not** change, and is the documented limit rather than a defect: a dependent keeps
answering until the restart, because an already-running actor does not consult its dependencies per
call — like a process holding a handle to a deleted file. And `enabled` stays `true`, deliberately:
it is the operator's intent, so the package starts again by itself once the dependency returns.

### What this cost the check itself, which is the reusable part

The first version of this note read `enabled` and called `true` "the node believes a broken package
is working". That was **the wrong observable**, and it would have gone on printing after the fix
landed.

`enabled` is the operator's *intent* — "I want this running" — and core deliberately does **not**
clear it, so the package starts again by itself once the dependency returns. Health is a different
fact. And an already-running actor does not consult its dependencies per call, so it finishes its
life either way, like a process holding a handle to a deleted file.

So the observable that separates fixed from not is **whether a restart now refuses to start it** —
which no single process can assert across. The leg now reports `enabled` and `answers` as two facts,
states what it cannot see, and names the two-script sequence for the rest:

```bash
# after 13 has uninstalled the leaf, restart the node, then:
CYCLE_FQID=devbot.dia.lib python 04-verify-serves.py    # must NOT answer once core's fix ships
```

A check that reads a flag adjacent to the thing it cares about is a check that keeps passing, or
keeps failing, through the change it was written to detect.

## What the closure legs found on their first run

The registry served a plan that **was not installable in the order it gave**. `app` declares `lib`
and `util`; `lib` also declares `util`:

```
       app ────────┐
        │          │
        ▼          ▼
       lib ─────▶ util
```

The closure walk is breadth-first and recorded the depth of the **first** arrival, so `util` —
reachable straight from `app` and again through `lib` — came out at depth 1, tied with the `lib` that
needs it, and the tie fell to frontier order: `lib, util, app`. The marketplace installs straight down
that list.

**The registry's own suite was green, and its closure test is a chain.** In a chain the depths are
0/1/2 — distinct — so first-arrival depth and install order agree, and the test passes under a rule
that is wrong. A diamond is the smallest graph where a node's shortest and longest paths differ, which
is exactly what a first-arrival depth cannot see. Two packages cannot show it; three in a line cannot
either.

Fixed in the registry (`depth` is now the **longest** path from the root, so the existing
deepest-first sort is a topological order by construction) with both shapes pinned as tests, each
shown to fail against the old rule.

The second failure was in the same family and worse. **The walk visits each namespace once, keyed by
name**, so every later arrival was dropped — *range and all*. With `app` asking for `util@^0.1`, its
`lib` asking for `util@^0.2`, and both minors published, the registry answered `util 0.1.x`,
`unresolved_reason: null`, and a complete-looking plan. A node installing that gets a `lib` whose
declared requirement is not met, with nothing anywhere having said so.

A wrong **order** is at least a wrong answer to a question that was asked. This was a wrong answer
presented as a correct one. It now refuses, naming both asks, and the marketplace stops the install
with nothing landing — which also showed the marketplace refuses on the *field* rather than on the
one cause it was written against.

**Every live install before this one was a single package declaring nothing.** The closure lane on
both sides was fully unit-tested and had never once been handed a graph.

## A script that goes red for the wrong reason

`03-grant-and-call.py` grants, uninstalls and reinstalls, so a second run against one fqid finds a
package that is installed-but-not-running and gets `ServiceUnavailable` where it expects
`AccessDenied`. The first version called that two failures and described the enforcer — about a script
that had simply already been run.

A suite that goes red for reasons unrelated to the thing it tests is a suite people stop reading,
which is the state that hides a real failure. It now refuses to start and names what it needs.

## Reading the node's log, which several of these checks depend on

Some evidence is only in the boot log — `DependencyUnmet` is not reachable over the bus, because the
`package.error` event has already fired by the time a script connects.

**`grep -c "kind=version_mismatch"` returns 0 while the lines are right there.** The log renders
field names with ANSI escapes between the token and its `=`, so `kind=` is not the string in the
stream. Grep the *value* alone:

```bash
docker logs waffler-beta 2>&1 | grep DependencyUnmet
docker logs waffler-beta 2>&1 | grep version_mismatch
```

Worth knowing before concluding a path did not fire. **A zero from a grep that could never match
reads exactly like a zero from a check that ran** — the same shape as the rest of this file.

And the ANSI escapes are only one cause of it. Within an hour of writing the line above I believed a
comment had been deleted from a shared file, because I grepped a sentence I had written in capitals
using lower case. Same failure, different reason, and the reason does not matter: **a search that
returns nothing has two explanations and the tool reports one of them.**

So: **a grep used to establish a NEGATIVE needs a positive control in the same run.** Three different
causes have produced the same misleading zero here in one day — ANSI escapes between a field name and
its `=`, letter case, and an identifier the code never names. Grepping for something you know is
present proves the pattern and the stream are both what you think:

```bash
# "the gate did not fire" is only worth reading if this line also appears
docker logs waffler-beta 2>&1 | grep -c "packages provisioning"   # must be > 0
docker logs waffler-beta 2>&1 | grep -c DependencyUnmet
```

And a probe has **three** outcomes, not two. `grep` says so in its exit code and a count discards it:

```
grep -ac <present> <file>   → 1, exit 0    found it
grep -ac <absent>  <file>   → 0, exit 1    LOOKED, and it is not there
grep -ac <any>     <gone>   →    exit 2    COULD NOT LOOK
```

Only the middle one is evidence. Treating the third as "not there" claims a measurement the run did
not make — which is the same defect as a check whose output asserts something it did not measure,
one layer down. **A probe that cannot report "I could not look" will report "it is not there."**

## `AccessDenied` cannot tell "not granted" from "not running"

`03-grant-and-call.py` reads `AccessDenied` on its baseline call as "installed, running, and not yet
granted" — which is what it usually means, and it is the precondition the whole script needs.

It is not sufficient. **The enforcer answers before the router does**, so a package whose actor has
been dropped is refused with `AccessDenied` too, and the two are indistinguishable from outside
until a grant takes the enforcer out of the path. Found by running the script against a package that
had been reinstalled without a restart: step 1 passed, the grant went in, and step 4 came back
`ServiceUnavailable: the package actor has been dropped`.

So the precondition is now checked at the only place it can be — **after** the grant, where the
answer is no longer ambiguous — and it exits as a skip naming what it needs rather than reporting a
failure about an enforcer.

The general shape is worth more than the fix: **a check placed before a gate can only see what the
gate lets through.** Asking what a check READ, rather than what it covers, is what surfaces it.

## One assertion that was wrong, and why it is worth recording

An earlier version of the uninstall leg asserted "it must stop answering" and passed — while the
package had never answered in that process, because it was installed and not yet running. The
assertion was true and proved nothing: the observable was right and the reason was not. `05-uninstall.py`
now refuses to continue unless the baseline call succeeds first.
