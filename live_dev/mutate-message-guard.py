#!/usr/bin/env python3
"""Neutralise every clause of the message-shape guard, one at a time, and report which survive.

Core's generalisation, and the reason this exists: mutating a clause to `false` proves it CAN block;
mutating it so it never blocks proves it is the one DOING the blocking. Only the second direction can
find a clause that is redundant — one the suite would stay green without.

Run from a crate root. Reverts after each mutation whether or not the test ran.

## THREE WAYS THIS SCRIPT CAN LIE, AND WHAT CATCHES EACH

1. AN ANCHOR STOPS MATCHING after a refactor. The clause is never mutated, the test passes, and the
   run reads as a clean sweep. Caught: a non-matching anchor is NOT APPLIED, exits 2, and the count
   of clauses actually exercised is printed before any verdict — "no survivors" over four of nine
   reads identically to "no survivors" over nine.

2. AN ANCHOR MATCHES TWICE. `replace(..., 1)` changes the first only, the other copy keeps the clause
   live, and the sweep reports it killed for a reason unrelated to the clause. Caught by requiring
   uniqueness.

3. AN ANCHOR MATCHES THE WRONG PLACE — unique, applied, and not the clause intended. Found by
   self-testing this script: breaking one anchor to a string that happened to exist elsewhere
   produced a confident SURVIVED for a clause that was never touched.

   NOTHING HERE CATCHES THAT, and it cannot be caught mechanically — a substring is a substring. What
   limits the damage is that a survivor is a FAILURE (exit 1), so it must be investigated rather than
   read. A survivor is a question, never a finding.
"""

import io
import subprocess
import sys

PATH = "src/message_shape.test.rs"

# (label, exact source text, replacement that NEUTRALISES the clause)
#
# In the `if A || B || C { continue; }` guard a clause is neutralised by making it `false` — it stops
# causing a skip. In the `if X && Y && Z` decision it is neutralised by `true`. Both mean the same
# thing: this clause now excludes nothing.
MUTATIONS = [
    ("predicate: run-length floor",
     "if i - start < RUN || start == 0 || i >= bytes.len() {",
     "if false || start == 0 || i >= bytes.len() {"),
    ("predicate: leading-indentation exclusion",
     "if i - start < RUN || start == 0 || i >= bytes.len() {",
     "if i - start < RUN || false || i >= bytes.len() {"),
    ("predicate: trailing-run exclusion",
     "if i - start < RUN || start == 0 || i >= bytes.len() {",
     "if i - start < RUN || start == 0 || false {"),
    ("predicate: before-is-alphanumeric",
     "if before.is_ascii_alphanumeric() && after != b'{' && after != b'=' {",
     "if true && after != b'{' && after != b'=' {"),
    ("predicate: placeholder exclusion",
     "if before.is_ascii_alphanumeric() && after != b'{' && after != b'=' {",
     "if before.is_ascii_alphanumeric() && true && after != b'=' {"),
    ("predicate: aligned-assignment exclusion",
     "if before.is_ascii_alphanumeric() && after != b'{' && after != b'=' {",
     "if before.is_ascii_alphanumeric() && after != b'{' && true {"),
    ("walker: recursion into subdirectories",
     "rust_sources(&path, out);",
     "let _ = &path;"),
    ("filter: comment exclusion",
     'if line.trim_start().starts_with("//") {',
     "if false {"),
    ("filter: string-literal requirement",
     "    line.contains(char::from(34))",
     "    true"),
]


def run(label, frm, to):
    original = io.open(PATH, encoding="utf-8", newline="").read()
    # A MUTATION THAT DID NOT APPLY PRODUCES A PASSING TEST THAT READS EXACTLY LIKE EVIDENCE, and it is
    # the failure this whole script is most likely to have. An anchor stops matching after any
    # refactor of the file it targets; the run then reports a clean sweep having exercised nothing.
    #
    # So a missing anchor is a FAILURE of the sweep, never a skipped line — see the exit below.
    occurrences = original.count(frm)
    if occurrences == 0:
        print(f"  NOT APPLIED  {label}  <-- the anchor no longer matches; this clause was NOT tested")
        return None
    # AND IT MUST BE UNIQUE. `replace(..., 1)` changes the first occurrence only, so a duplicated
    # anchor leaves the other copy intact — the clause is still live, the test still passes, and the
    # sweep reports it killed for a reason that has nothing to do with the clause.
    if occurrences > 1:
        print(f"  NOT APPLIED  {label}  <-- the anchor matches {occurrences} places; it must be unique")
        return None

    io.open(PATH, "w", encoding="utf-8", newline="").write(original.replace(frm, to, 1))
    try:
        out = subprocess.run(
            ["cargo", "test", "--quiet", "message_shape"], capture_output=True, text=True
        )
        killed = "FAILED" in (out.stdout + out.stderr)
    finally:
        # Reverted whether or not the run happened. A sweep that dies mid-run and leaves a mutation in
        # place is a guard silently weakened by the thing built to check it — and the tree stays green,
        # which is what makes it nasty.
        io.open(PATH, "w", encoding="utf-8", newline="").write(original)
    return killed


print("Neutralising each clause; a SURVIVOR is a clause nothing proves is needed.\n")
survivors = []
not_applied = []
for label, frm, to in MUTATIONS:
    killed = run(label, frm, to)
    if killed is None:
        not_applied.append(label)
        continue
    print(f"  {'KILLED  ' if killed else 'SURVIVED'}  {label}")
    if not killed:
        survivors.append(label)

exercised = len(MUTATIONS) - len(not_applied)
print(f"\n{exercised} of {len(MUTATIONS)} clauses exercised.")

# THE COUNT IS REPORTED BEFORE THE VERDICT, deliberately. "No survivors" over four of nine clauses
# reads identically to "no survivors" over nine, and the first is the result of a broken sweep.
if not_applied:
    print(f"\n{len(not_applied)} mutation(s) NEVER APPLIED, so those clauses are untested:")
    for label in not_applied:
        print(f"  - {label}")
    print("\nFix the anchors before reading anything else here as a result.")
    sys.exit(2)

if survivors:
    print(f"\n{len(survivors)} clause(s) nothing can kill:")
    for label in survivors:
        print(f"  - {label}")
    sys.exit(1)

print("every clause is load-bearing and something proves it")
