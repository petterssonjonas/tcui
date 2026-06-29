#!/usr/bin/env bash
set -euo pipefail

repo="${TCUI_REPO:-petterssonjonas/TermChatUI}"
version="${TCUI_VERSION:-latest}"
bin_dir="${TCUI_BIN_DIR:-$HOME/.local/bin}"
target="x86_64-unknown-linux-gnu"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "tcui installer currently supports Linux." >&2
  exit 1
fi

if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "tcui installer currently publishes x86_64 Linux binaries." >&2
  exit 1
fi

if [[ "$version" == "latest" ]]; then
  url="https://github.com/${repo}/releases/latest/download/tcui-${target}.tar.gz"
else
  url="https://github.com/${repo}/releases/download/${version}/tcui-${target}.tar.gz"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$bin_dir"
curl -fsSL "$url" -o "$tmp/tcui.tar.gz"
if [[ "$version" == "latest" ]]; then
  sums_url="https://github.com/${repo}/releases/latest/download/SHA256SUMS"
else
  sums_url="https://github.com/${repo}/releases/download/${version}/SHA256SUMS"
fi
curl -fsSL "$sums_url" -o "$tmp/SHA256SUMS"
grep "tcui-${target}.tar.gz\$" "$tmp/SHA256SUMS" > "$tmp/SHA256SUMS.selected"
(cd "$tmp" && sha256sum -c SHA256SUMS.selected)
tar -xzf "$tmp/tcui.tar.gz" -C "$tmp"
install -m 755 "$tmp/tcui" "$bin_dir/tcui"
echo "tcui installed to $bin_dir/tcui"
