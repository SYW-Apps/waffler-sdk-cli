#!/usr/bin/env python3
"""Verify the countersignature chain on an artifact the registry actually served.

## WHY AN INDEPENDENT DECODER

Two of the three implementations of this frame — the CLI's producer and the registry's countersigner —
were written in this session. A verifier sharing either one's code would agree with it by construction.
This decodes the trailer from raw bytes with a third implementation and checks the maths itself, so a
pass means three encoders agree rather than one agreeing with itself.

STEP 0 PROVES THIS SCRIPT FIRST. It reproduces the committed interop vector byte for byte before
anything else runs. A Python trailer that disagreed with the format would make every assertion below a
statement about this script rather than about the artifact.
"""

import hashlib
import struct
import sys

import msgpack
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

MAGIC = b"WFLRSIG\x01"
FOOTER = 12  # u32 length + 8-byte magic

failures = []


def step(t):
    print(f"\n\033[36m==== {t} ====\033[0m", flush=True)


def ok(m):
    print(f"\033[32m  OK    {m}\033[0m", flush=True)


def bad(m):
    failures.append(m)
    print(f"\033[31m  FAIL  {m}\033[0m", flush=True)


def interop_vector_reproduces():
    """The committed vector: one Registry signature, key = 32x0x01, signature = 64x0x02."""
    trailer = {
        "signatures": [
            {"role": "Registry", "public_key": bytes([0x01] * 32), "signature": bytes([0x02] * 64)}
        ]
    }
    encoded = msgpack.packb(trailer, use_bin_type=True)
    expected = bytes.fromhex(
        "81aa7369676e61747572657391"
        "83a4726f6c65a85265676973747279"
        "aa7075626c69635f6b6579c420" + "01" * 32 +
        "a97369676e6174757265c440" + "02" * 64
    )
    return encoded, expected


def split(artifact):
    """Payload, trailer. The length and the magic sit at the very end, so the frame is read backwards."""
    if artifact[-8:] != MAGIC:
        raise SystemExit("the artifact does not end with the trailer magic")
    declared = struct.unpack("<I", artifact[-FOOTER:-8])[0]
    body = artifact[-FOOTER - declared : -FOOTER]
    payload = artifact[: -FOOTER - declared]
    return payload, msgpack.unpackb(body, raw=False, strict_map_key=False)


def main(path):
    step("0. this script reproduces the committed interop vector, or nothing below means anything")
    encoded, expected = interop_vector_reproduces()
    if encoded != expected:
        bad(f"this decoder disagrees with the format:\n    got      {encoded.hex()}\n    expected {expected.hex()}")
        print("\033[31mstopping: every assertion below would be about this script\033[0m")
        sys.exit(1)
    ok("byte for byte — three encoders now agree on this shape")

    artifact = open(path, "rb").read()
    payload, trailer = split(artifact)
    sigs = trailer["signatures"]
    print(f"  artifact {len(artifact)} bytes, payload {len(payload)}, {len(sigs)} signature(s)")

    step("1. the chain is publisher then registry, in that order")
    roles = [s["role"] for s in sigs]
    if roles == ["Publisher", "Registry"]:
        ok("Publisher, Registry")
    else:
        bad(f"the chain is {roles}")
        sys.exit(1)

    step("2. the publisher signature covers the ARCHIVE")
    pub = sigs[0]
    try:
        Ed25519PublicKey.from_public_bytes(pub["public_key"]).verify(pub["signature"], payload)
        ok(f"verified against the carried key {pub['public_key'].hex()[:16]}...")
    except InvalidSignature:
        bad("the publisher signature does not verify over the payload")

    step("3. the registry signature covers the archive AND the publisher's signature")
    # THIS IS THE WHOLE POINT OF THE CHAIN. Coverage is structural: signatures[i] covers
    # zip || signatures[0..i].signature. If the registry had signed the payload ALONE, its
    # signature would still verify over the payload — and the publisher signature beneath it would be
    # swappable for anyone else's while the registry's endorsement kept verifying.
    reg = sigs[1]
    message = payload + pub["signature"]
    try:
        Ed25519PublicKey.from_public_bytes(reg["public_key"]).verify(reg["signature"], message)
        ok("verified over payload || publisher_signature — it commits to the publisher beneath it")
    except InvalidSignature:
        bad("the registry signature does not cover the publisher signature")

    step("4. and it does NOT verify over the payload alone")
    # The negative half. Without it, a registry that signed only the payload would pass step 3 if
    # step 3 were written slightly wrong, and the swappability would survive undetected.
    try:
        Ed25519PublicKey.from_public_bytes(reg["public_key"]).verify(reg["signature"], payload)
        bad("the registry signature verifies over the payload ALONE — the publisher is swappable")
    except InvalidSignature:
        ok("it does not, which is what makes the publisher signature unswappable")

    step("5. the payload is a readable archive, unchanged by any of it")
    import io
    import zipfile

    with zipfile.ZipFile(io.BytesIO(payload)) as z:
        names = z.namelist()
        if ".manifest" in names:
            ok(f"{len(names)} entries, including /.manifest")
        else:
            bad(f"no /.manifest in {names}")
        # The archive's own bytes must be untouched: every signature covers them, so anything that
        # re-zipped or normalised the payload would produce an artifact a node refuses as corrupt.
        digest = hashlib.sha256(payload).hexdigest()
        print(f"  payload sha256 {digest}")

    print("\n" + "=" * 70)
    if failures:
        print(f"\033[31m{len(failures)} FAILURE(S)\033[0m")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\033[32mthe countersignature chain is correct\033[0m")


main(sys.argv[1] if len(sys.argv) > 1 else "/out/signed-1.zip")
