#!/usr/bin/env bash
# A successful release build must validate the new artifact, not a stale default-path binary.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
stale="$(realpath "${1:?usage: release-build.sh EXISTING_GNU_BINARY}")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
project="$work/project"
binary="$project/target/x86_64-unknown-linux-gnu/release/tcui"
mkdir -p "$project/src" "$project/scripts" "$(dirname "$binary")"
cp "$stale" "$binary"
cp scripts/build-release.sh scripts/check-release-abi.py "$project/scripts/"
cat > "$project/Cargo.toml" <<'TOML'
[package]
name = "tcui"
version = "0.0.0"
edition = "2024"
TOML
cat > "$project/src/main.rs" <<'RUST'
fn main() {
    println!("fresh-build");
}
RUST
cargo generate-lockfile --offline --manifest-path "$project/Cargo.toml"
env -u CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR="$work/other-target" \
  bash "$project/scripts/build-release.sh"
if [[ "$("$binary" --help)" != fresh-build ]]; then
  echo 'Release build validated a stale binary instead of the newly compiled artifact.' >&2
  exit 1
fi
for flag in CFLAGS_x86_64-unknown-linux-gnu TARGET_CFLAGS; do
  if env -u CARGO_TARGET_DIR "$flag=-march=x86-64-v3" \
    bash "$project/scripts/build-release.sh" > "$work/flags.log" 2>&1; then
    printf 'Release build accepted unsafe compiler override %s.\n' "$flag" >&2
    exit 1
  else
    status="$?"
    [[ "$status" == 2 ]] || { cat "$work/flags.log" >&2; exit 1; }
  fi
done
printf 'Release target-directory and compiler-override guards passed.\n'
