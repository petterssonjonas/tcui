#!/usr/bin/env bash
set -euo pipefail

# Exercise the built CLI without opening a TUI, contacting providers, or using real data.
binary="$(realpath "${1:-target/debug/tcui}")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export HOME="$work/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_CACHE_HOME="$HOME/.cache"
mkdir -p "$HOME"
"$binary" --help
"$binary" auth --help
output="$("$binary" </dev/null)"
[[ "$output" == 'TermChatUI requires an interactive terminal.' ]]
