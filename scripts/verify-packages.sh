#!/usr/bin/env bash
# Inspect native metadata and run every packaged binary without system installation.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
target="$(realpath "${CARGO_TARGET_DIR:-target}")"
binary="$target/x86_64-unknown-linux-gnu/release/tcui"
dist="$target/dist"
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
python3 scripts/check-release-abi.py "$binary"
(cd "$dist" && sha256sum --check --strict SHA256SUMS)

[[ "$(dpkg-deb --field "$dist/tcui_${version}_amd64.deb" Package)" == tcui ]]
[[ "$(dpkg-deb --field "$dist/tcui_${version}_amd64.deb" Version)" == "$version-1" ]]
[[ "$(dpkg-deb --field "$dist/tcui_${version}_amd64.deb" Architecture)" == amd64 ]]
dependencies="$(dpkg-deb --field "$dist/tcui_${version}_amd64.deb" Depends)"
DEB_DEPENDENCIES="$dependencies" python3 - <<'PY'
import os
import sys
if {item.strip() for item in os.environ["DEB_DEPENDENCIES"].split(",")} != {
    "libc6 (>= 2.34)", "libgcc-s1", "ca-certificates",
}:
    sys.exit("unexpected Debian runtime dependencies")
PY
dpkg-deb --extract "$dist/tcui_${version}_amd64.deb" "$work/deb"
[[ "$(rpm -qp --queryformat '%{NAME} %{VERSION} %{RELEASE} %{ARCH}' "$dist/tcui-${version}-1.x86_64.rpm")" == "tcui $version 1 x86_64" ]]
RPM_DEPENDENCIES="$(rpm -qp --requires "$dist/tcui-${version}-1.x86_64.rpm")" python3 - <<'PY'
import os
import sys
if {item.strip() for item in os.environ["RPM_DEPENDENCIES"].splitlines()
        if not item.startswith("rpmlib(")} != {
    "glibc(x86-64) >= 2.34", "libgcc_s.so.1(GCC_4.2.0)(64bit)", "ca-certificates",
}:
    sys.exit("unexpected RPM runtime dependencies")
PY
mkdir "$work/rpm" "$work/tar"
(cd "$work/rpm" && rpm2cpio "$dist/tcui-${version}-1.x86_64.rpm" | cpio --extract --make-directories --quiet)
for format in deb rpm; do
  cmp LICENSE "$work/$format/usr/share/doc/tcui/LICENSE"
  cmp assets/models/potion-base-8M/LICENSE "$work/$format/usr/share/doc/tcui/potion-base-8M-LICENSE"
  cmp "$binary" "$work/$format/usr/bin/tcui"
  bash evals/smoke.sh "$work/$format/usr/bin/tcui"
done

tar -xzf "$dist/tcui-x86_64-unknown-linux-gnu.tar.gz" -C "$work/tar"
export TCUI_BIN_DIR="$work/local user/bin"
bash "$work/tar/install.sh"
cmp "$binary" "$TCUI_BIN_DIR/tcui"
bash evals/smoke.sh "$TCUI_BIN_DIR/tcui"
bash "$work/tar/install.sh" uninstall
test ! -e "$TCUI_BIN_DIR/tcui"

# Exercise the generated Arch recipe's checksums and package function offline.
(
  # shellcheck disable=SC1091
  source "$dist/PKGBUILD"
  # The generated recipe declares this array; ShellCheck cannot resolve its path.
  # shellcheck disable=SC2154
  printf '%s  %s\n' "${sha256sums[0]}" "$dist/tcui-x86_64-unknown-linux-gnu.tar.gz" \
    "${sha256sums[1]}" "$dist/LICENSE" "${sha256sums[2]}" "$dist/potion-base-8M-LICENSE" | sha256sum --check --strict
  export srcdir="$work/tar" pkgdir="$work/arch"
  package
)
cmp LICENSE "$work/arch/usr/share/licenses/tcui-bin/LICENSE"
cmp assets/models/potion-base-8M/LICENSE "$work/arch/usr/share/licenses/tcui-bin/potion-base-8M-LICENSE"
cmp "$binary" "$work/arch/usr/bin/tcui"
bash evals/smoke.sh "$work/arch/usr/bin/tcui"

# Source distribution must identify the same commit as the binary build checkout.
gzip -dc "$dist/tcui-${version}-source.tar.gz" > "$work/source.tar"
[[ "$(git get-tar-commit-id < "$work/source.tar")" == "$(git rev-parse HEAD)" ]]
printf 'Package metadata, checksums, payloads, Arch recipe, and local install/uninstall passed.\n'
