#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
mode="${1:-development}"
case "$mode" in development|full) ;; *) echo 'usage: ci.sh [development|full]' >&2; exit 2 ;; esac

# Existing restart/confinement tests require sibling HOME and XDG directories.
# Preserve Rust's tool/cache locations while isolating application state.
run_tests() (
  sandbox="$(mktemp -d)"
  trap 'rm -rf "$sandbox"' EXIT
  export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
  export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
  export HOME="$sandbox/home" XDG_CONFIG_HOME="$sandbox/config"
  export XDG_DATA_HOME="$sandbox/data" XDG_CACHE_HOME="$sandbox/cache"
  mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME"
  cargo test --locked --all-targets "$@"
)

# Keep a single command contract shared by PR checks and tagged releases.
jactionlint
shellcheck scripts/ci.sh scripts/setup-ci.sh scripts/release-ref.sh scripts/package.sh \
  scripts/build-release.sh scripts/smoke-oldest-runtime.sh scripts/verify-packages.sh \
  scripts/install.sh scripts/scan-secrets.sh evals/smoke.sh tests/install.sh tests/release-abi.sh \
  tests/release-build.sh
bash tests/install.sh
bash tests/release-abi.sh
python3 -B scripts/test_notify_ping.py
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
run_tests --all-features
cargo build --locked --all-features
bash evals/smoke.sh "${CARGO_TARGET_DIR:-target}/debug/tcui"

if [[ "$mode" == full ]]; then
  cargo clippy --locked --all-targets --no-default-features -- -D warnings
  run_tests --no-default-features
  RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
  bash scripts/build-release.sh
  bash tests/release-build.sh "${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/tcui"
  bash evals/smoke.sh "${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-gnu/release/tcui"
  bash scripts/package.sh
  bash scripts/verify-packages.sh
fi
