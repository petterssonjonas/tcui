#!/usr/bin/env bash
# Install the same mise-pinned tools used in Toolbx; never edit project pins here.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mode="${1:-development}"
case "$mode" in development|full) ;; *) echo 'usage: setup-ci.sh [development|full]' >&2; exit 2 ;; esac
[[ "$(uname -s)/$(uname -m)" == Linux/x86_64 ]] || { echo 'CI tools require Linux x86-64' >&2; exit 1; }
tools=(rust jactionlint shellcheck)
if [[ "$mode" == full ]]; then
  tools+=(zig cargo:cargo-zigbuild cargo:cargo-deb cargo:cargo-generate-rpm)
fi
mise install "${tools[@]}"
