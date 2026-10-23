#!/usr/bin/env bash
# Run the published payload on glibc 2.34; compilation stays outside containers.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
chmod 755 "$work"
tar -xzf "${CARGO_TARGET_DIR:-target}/dist/tcui-x86_64-unknown-linux-gnu.tar.gz" -C "$work" tcui
cp evals/smoke.sh "$work/smoke.sh"
chmod 644 "$work/smoke.sh"
# Relabel only our scratch payload for SELinux, never files in the checkout.
# Pin the runtime image, disable networking and writes, and never expose user data.
# Expansion belongs to the container shell, not the build host.
# shellcheck disable=SC2016
"${TCUI_CONTAINER_ENGINE:-docker}" run --rm --platform linux/amd64 \
  --network none --read-only --tmpfs /tmp:rw,nosuid,nodev \
  --user 65534:65534 --cap-drop ALL --security-opt no-new-privileges \
  --volume "$work:/release:ro,Z" \
  quay.io/rockylinux/rockylinux:9@sha256:8101994123cf3d0a8fee517bee7f39e555c7d92bd2d9eb3303cc988a0eeed00f \
  bash -euo pipefail -c '
    test "$(getconf GNU_LIBC_VERSION)" = "glibc 2.34"
    rpm -q glibc libgcc ca-certificates
    bash /release/smoke.sh /release/tcui
  '
