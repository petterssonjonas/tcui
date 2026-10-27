#!/usr/bin/env python3
"""Validate the native release's loader, libraries, and required symbol versions."""

import os
from pathlib import Path
import re
import subprocess
import sys


GLIBC = (2, 34)
GCC = (4, 2, 0)
LIBRARIES = {
    "libc.so.6", "libm.so.6", "libpthread.so.0", "libdl.so.2", "librt.so.1",
    "libutil.so.1", "libresolv.so.2", "libgcc_s.so.1", "ld-linux-x86-64.so.2",
}


def newer(version, baseline):
    parts = tuple(int(part) for part in version.split("."))
    width = max(len(parts), len(baseline))
    return parts + (0,) * (width - len(parts)) > baseline + (0,) * (width - len(baseline))


def check_report(report):
    if "[Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]" not in report:
        raise ValueError("release must use the x86-64 glibc dynamic loader")
    libraries = set(re.findall(r"\(NEEDED\).*?\[([^\]]+)\]", report))
    if "libc.so.6" not in libraries or libraries - LIBRARIES:
        raise ValueError(f"unsupported release libraries: {sorted(libraries)}")
    versions = re.findall(r"Name: ((?:GLIBC|GCC)_\S+)\s+Flags: (\S+)", report)
    if not any(name.startswith("GLIBC_") for name, _ in versions):
        raise ValueError("missing glibc version requirements")
    for name, flags in versions:
        family, version = name.split("_", 1)
        if not re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", version):
            raise ValueError(f"unsupported runtime ABI: {name}")
        baseline = GLIBC if family == "GLIBC" else GCC
        # The loader checks version-need flags, not just a symbol's weak binding.
        if flags != "WEAK" and newer(version, baseline):
            raise ValueError(f"required runtime ABI exceeds release baseline: {name}")
    for line in report.splitlines():
        fields = line.split()
        if len(fields) < 8 or fields[6] != "UND" or fields[4] == "WEAK":
            continue
        match = re.search(r"@((?:GLIBC|GCC)_([0-9.]+))", fields[7])
        if match:
            name, version = match.groups()
            baseline = GLIBC if name.startswith("GLIBC_") else GCC
            if newer(version, baseline):
                raise ValueError(f"strong import exceeds release baseline: {fields[7]}")


def check(binary):
    with binary.open("rb") as source:
        header = source.read(20)
    if len(header) != 20 or header[:6] != b"\x7fELF\x02\x01" or header[18:20] != b"\x3e\x00":
        raise ValueError("release must be a little-endian x86-64 ELF binary")
    report = subprocess.run(
        ["readelf", "--wide", "--program-headers", "--dynamic", "--version-info", "--dyn-syms", str(binary)],
        check=True, capture_output=True, text=True, env={**os.environ, "LC_ALL": "C"},
    ).stdout
    check_report(report)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: check-release-abi.py BINARY")
    try:
        check(Path(sys.argv[1]))
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"release ABI: {error}")
    print(f"Release ABI passed (glibc <= 2.34, GCC <= 4.2.0): {sys.argv[1]}")
