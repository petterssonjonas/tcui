#!/usr/bin/env bash
# Parser regressions: optional weak imports must not become hard runtime floors.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
python3 - <<'PY'
import runpy
import unittest

check = runpy.run_path("scripts/check-release-abi.py")["check_report"]


def report(version="2.34", flags="none", binding="GLOBAL", library="libc.so.6"):
    return f"""
      [Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]
  0x0000000000000001 (NEEDED) Shared library: [{library}]
  0x0010: Name: GLIBC_{version}  Flags: {flags}  Version: 2
     1: 0000000000000000     0 FUNC    {binding} DEFAULT  UND example@GLIBC_{version} (2)
"""


class ReleaseAbi(unittest.TestCase):
    def test_baseline_and_numeric_order(self):
        check(report())
        check(report("2.9"))
        check(report() + "  Name: GCC_4.2.0 Flags: none Version: 3\n")

    def test_required_newer_version(self):
        with self.assertRaisesRegex(ValueError, "required runtime ABI"):
            check(report("2.38"))

    def test_optional_newer_version(self):
        check(report("2.38", "WEAK", "WEAK"))

    def test_weak_symbol_with_hard_version_requirement(self):
        with self.assertRaisesRegex(ValueError, "required runtime ABI"):
            check(report("2.38", "none", "WEAK"))

    def test_strong_symbol_with_weak_version_requirement(self):
        with self.assertRaisesRegex(ValueError, "strong import"):
            check(report("2.38", "WEAK", "GLOBAL"))

    def test_unsupported_library(self):
        with self.assertRaisesRegex(ValueError, "unsupported release libraries"):
            check(report() + "  (NEEDED) Shared library: [libstdc++.so.6]\n")

    def test_unsupported_abi(self):
        for version in ("PRIVATE", "ABI_DT_RELR"):
            with self.subTest(version=version), self.assertRaisesRegex(ValueError, "unsupported runtime ABI"):
                check(report(version))
        with self.assertRaisesRegex(ValueError, "required runtime ABI"):
            check(report() + "  Name: GCC_12.0.0 Flags: none Version: 3\n")

    def test_missing_dynamic_runtime(self):
        with self.assertRaisesRegex(ValueError, "dynamic loader"):
            check("")
        with self.assertRaisesRegex(ValueError, "missing glibc"):
            check("[Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]\n"
                  "(NEEDED) Shared library: [libc.so.6]\n")


unittest.main()
PY
