#!/usr/bin/env python3
"""Neutralise every clause of the message-shape guard, one at a time, and report which survive.

Core's generalisation, and the reason this exists: mutating a clause to `false` proves it CAN block;
mutating it so it never blocks proves it is the one DOING the blocking. Only the second direction can
find a clause that is redundant — one the suite would stay green without.

Run from a crate root. Reverts after each mutation whether or not the test ran.
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
    if frm not in original:
        print(f"  ANCHOR MISSING  {label}")
        return None
    io.open(PATH, "w", encoding="utf-8", newline="").write(original.replace(frm, to, 1))
    try:
        out = subprocess.run(
            ["cargo", "test", "--quiet", "message_shape"], capture_output=True, text=True
        )
        killed = "FAILED" in (out.stdout + out.stderr)
    finally:
        io.open(PATH, "w", encoding="utf-8", newline="").write(original)
    return killed


print("Neutralising each clause; a SURVIVOR is a clause nothing proves is needed.\n")
survivors = []
for label, frm, to in MUTATIONS:
    killed = run(label, frm, to)
    if killed is None:
        continue
    print(f"  {'KILLED  ' if killed else 'SURVIVED'}  {label}")
    if not killed:
        survivors.append(label)

print()
if survivors:
    print(f"{len(survivors)} clause(s) nothing can kill:")
    for s in survivors:
        print(f"  - {s}")
    sys.exit(1)
print("every clause is load-bearing and something proves it")
