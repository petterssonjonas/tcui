#!/usr/bin/env bash
# Scan all history reachable from this checkout, including merge-resolution patches.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
[[ "$(uname -s)/$(uname -m)" == Linux/x86_64 ]] || { echo 'Scanner requires Linux x86-64' >&2; exit 1; }
[[ "$(git rev-parse --is-shallow-repository)" == false ]] || { echo 'Secret scanning requires fetch-depth: 0' >&2; exit 1; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL \
  https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_linux_x64.tar.gz \
  -o "$work/gitleaks.tar.gz"
(
  cd "$work"
  printf '%s\n' '551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb  gitleaks.tar.gz' | sha256sum --check --strict
  tar -xzf gitleaks.tar.gz gitleaks
)
"$work/gitleaks" git --redact --log-opts='--full-history --diff-merges=first-parent HEAD' .
