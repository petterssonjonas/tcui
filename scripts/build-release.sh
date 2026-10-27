#!/usr/bin/env bash
# Cross-link against Zig's glibc 2.34 sysroot, never the build host's libc.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if (( $# != 0 )); then
  echo 'Usage: bash scripts/build-release.sh' >&2
  exit 2
fi
[[ "$(uname -s)/$(uname -m)" == Linux/x86_64 ]] || { echo 'Release builds require Linux x86-64' >&2; exit 1; }
# Avoid inherited linker/CPU/header overrides silently changing the release ABI.
while IFS='=' read -r -d '' variable value; do
  case "$variable" in
    RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|CARGO_BUILD_RUSTFLAGS|\
    CARGO_TARGET_*_RUSTFLAGS|CARGO_TARGET_*_LINKER|\
    CC|CC_*|CXX|CXX_*|AR|AR_*|RANLIB|RANLIB_*|\
    CFLAGS|CFLAGS_*|CXXFLAGS|CXXFLAGS_*|CPPFLAGS|CPPFLAGS_*|LDFLAGS|LDFLAGS_*|\
    HOST_CC|HOST_CXX|HOST_AR|HOST_RANLIB|HOST_CFLAGS|HOST_CXXFLAGS|HOST_CPPFLAGS|HOST_LDFLAGS|\
    TARGET_CC|TARGET_CXX|TARGET_AR|TARGET_RANLIB|TARGET_CFLAGS|TARGET_CXXFLAGS|TARGET_CPPFLAGS|TARGET_LDFLAGS)
      if [[ -n "$value" ]]; then
        printf 'Release builds require %s unset; use an ordinary cargo build for custom flags.\n' "$variable" >&2
        exit 2
      fi
      ;;
  esac
done < <(env -0)
export RUSTFLAGS='-C target-cpu=x86-64'
target="${CARGO_TARGET_DIR:-target}"
cargo zigbuild --locked --release --all-features --target x86_64-unknown-linux-gnu.2.34 \
  --target-dir "$target"
python3 scripts/check-release-abi.py "$target/x86_64-unknown-linux-gnu/release/tcui"
