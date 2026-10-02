#!/usr/bin/env python3
"""Print what a bundle actually declares. Used to read back a pack rather than trust its exit code.

    python3 _read_bundle.py <bundle.zip>

A packer's zero exit says the process finished, not that the artifact says what was asked for. This
is a FILE rather than a heredoc because an inline script carrying quotes and escapes is how content
gets eaten on the way to disk — nine times in one session, by four different mechanisms.
"""
import json
import os
import sys
import zipfile

if len(sys.argv) != 2:
    print(__doc__)
    sys.exit(2)

path = sys.argv[1]
z = zipfile.ZipFile(path)
manifest = json.loads(z.read(".manifest"))
middleware = [d.get("id", "?") for d in manifest.get("middleware") or []]
artifacts = [(a.get("name"), a.get("kind")) for a in manifest.get("artifacts") or []]

print(f"  packed: {manifest['fqid']} {manifest['version']}")
print(f"  hosting_mode : {manifest.get('hosting_mode')}")
print(f"  artifacts    : {artifacts}")
print(f"  middleware   : {middleware or '(none)'}")
print(f"  entries      : {z.namelist()}")
print(f"  size         : {os.path.getsize(path)} bytes")

# A zip with no artifact entry would publish and install as a package that loads nothing - said
# rather than assumed, because the manifest listing one is a different fact from the zip carrying it.
present = [n for n in z.namelist() if n.startswith("artifact/") and n.strip("/") != "artifact"]
declared = {a[0] for a in artifacts}
missing = declared - {n.split("/", 1)[1] for n in present}
if missing:
    print(f"  FAIL: the manifest declares {sorted(missing)} and the zip carries {present}")
    sys.exit(1)
