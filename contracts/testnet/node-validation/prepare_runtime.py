#!/usr/bin/env python3
"""Prepare an isolated GLIBC overlay, never install packages on the host.

Run this download/extraction step separately from the network-disconnected suite.
The package pin was taken from Ubuntu noble-updates amd64 Packages metadata.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import urllib.request

PACKAGE = {
    "version": "2.39-0ubuntu8.9",
    "url": "https://archive.ubuntu.com/ubuntu/pool/main/g/glibc/libc6_2.39-0ubuntu8.9_amd64.deb",
    "sha256": "ff5557d99b51f761c4b7c92368b9cc45565eda17df9bf9eb4b134d09825008be",
    "bytes": 3264314,
}

def library_hashes(root):
    files = {}
    for path in sorted((root / "usr/lib/x86_64-linux-gnu").glob("*.so*")):
        if path.is_file() and not path.is_symlink():
            relative = str(path.relative_to(root))
            files[relative] = hashlib.sha256(path.read_bytes()).hexdigest()
    for name in ("libc.so.6", "libm.so.6", "ld-linux-x86-64.so.2"):
        if "usr/lib/x86_64-linux-gnu/" + name not in files:
            raise ValueError("Incomplete GLIBC package")
    return files

def validate_runtime(output):
    manifest = json.loads((output / "runtime.json").read_text())
    package = output / "libc6.deb"
    if manifest["package"] != PACKAGE or hashlib.sha256(package.read_bytes()).hexdigest() != PACKAGE["sha256"]:
        raise ValueError("Pinned libc6 package mismatch")
    # Derive hashes again from the pinned package; runtime.json is not its own
    # trust anchor and cannot authorize replacement libraries.
    with tempfile.TemporaryDirectory(prefix="nft-glibc-verify-") as tmp:
        subprocess.run(["dpkg-deb", "--extract", str(package), tmp], check=True)
        expected = library_hashes(Path(tmp))
    if manifest["files"] != expected or library_hashes(output / "root") != expected:
        raise ValueError("Extracted GLIBC differs from pinned package")
    return manifest

def prepare(output):
    output.mkdir(parents=True, exist_ok=False)
    with urllib.request.urlopen(PACKAGE["url"], timeout=60) as response:
        data = response.read(PACKAGE["bytes"] + 1)
    if len(data) != PACKAGE["bytes"] or hashlib.sha256(data).hexdigest() != PACKAGE["sha256"]:
        raise ValueError("Pinned libc6 package mismatch")
    package = output / "libc6.deb"
    package.write_bytes(data)
    subprocess.run(["dpkg-deb", "--extract", str(package), str(output / "root")], check=True)
    files = library_hashes(output / "root")
    (output / "runtime.json").write_text(json.dumps({"package": PACKAGE, "files": files}, indent=2) + "\n")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    prepare(parser.parse_args().output)
