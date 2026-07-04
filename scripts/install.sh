#!/usr/bin/env bash
# tcui installer: detects CPU arch and native package manager, downloads the
# matching release artifact, verifies its SHA256 against the release SHA256SUMS,
# and installs it.
#
# Override knobs:
#   TCUI_REPO        GitHub owner/repo (default: petterssonjonas/tcui)
#   TCUI_VERSION     release tag to install (default: latest)
#   TCUI_BIN_DIR     install dir for the tarball fallback (default: ~/.local/bin,
#                    or /usr/local/bin when run as root)
#   TCUI_PKG         force package type: deb | rpm | tarball (default: auto)
#
# Usage:
#   curl -fsSL https://github.com/petterssonjonas/tcui/releases/latest/download/install.sh | bash
#   TCUI_VERSION=v0.8.0 bash install.sh
set -euo pipefail

repo="${TCUI_REPO:-petterssonjonas/tcui}"
version="${TCUI_VERSION:-latest}"
force_pkg="${TCUI_PKG:-}"

err() { echo "tcui: $*" >&2; }
die() { err "$*"; exit 1; }

[[ "$(uname -s)" == "Linux" ]] || die "installer currently supports Linux only."

# --- arch detection -----------------------------------------------------------
case "$(uname -m)" in
  x86_64|amd64)  arch="x86_64"; rust_target="x86_64-unknown-linux-gnu"; deb_arch="amd64" ;;
  aarch64|arm64) arch="arm64";  rust_target="aarch64-unknown-linux-gnu"; deb_arch="arm64" ;;
  *)             die "unsupported architecture: $(uname -m)" ;;
esac

# --- package-manager detection (overridable) ---------------------------------
if [[ -n "$force_pkg" ]]; then
  pkg_type="$force_pkg"
  case "$pkg_type" in deb|rpm|tarball) ;; *) die "TCUI_PKG must be deb|rpm|tarball" ;; esac
elif command -v dpkg >/dev/null 2>&1; then
  pkg_type="deb"
elif command -v rpm >/dev/null 2>&1; then
  pkg_type="rpm"
else
  pkg_type="tarball"
fi

# rpm artifacts are published for x86_64 only; arm64 hosts with an rpm stack
# fall back to the tarball so the install still succeeds.
if [[ "$pkg_type" == "rpm" && "$arch" != "x86_64" ]]; then
  pkg_type="tarball"
fi

# --- resolve "latest" to a concrete tag via the GitHub API ------------------
if [[ "$version" == "latest" ]]; then
  tag="$(curl -fsSL "https://api.github.com/repos/${repo}/releases/latest" \
        | grep -m1 '"tag_name"' \
        | sed -E 's/^[[:space:]]*"tag_name":[[:space:]]*"v?([^"]+)".*/\1/')"
  [[ -n "$tag" ]] || die "could not resolve latest release tag from ${repo}"
else
  tag="${version#v}"
fi
base="https://github.com/${repo}/releases/download/v${tag}"

# --- pick the asset filename ------------------------------------------------
case "$pkg_type" in
  deb)     asset="tcui_${tag}_${deb_arch}.deb" ;;
  rpm)     asset="tcui-${tag}-1.x86_64.rpm" ;;
  tarball) asset="tcui-${rust_target}.tar.gz" ;;
esac

echo "tcui: installing ${asset} (pkg=${pkg_type}, arch=${arch}, tag=v${tag})"

# --- download + verify -------------------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL "${base}/${asset}" -o "${tmp}/${asset}"
asset_sum="$(sha256sum "${tmp}/${asset}" | awk '{print $1}')"

if curl -fsSL "${base}/SHA256SUMS" -o "${tmp}/SHA256SUMS"; then
  expected="$(grep -E "[[:space:]]${asset}\$" "${tmp}/SHA256SUMS" | awk '{print $1}')"
  [[ -n "$expected" ]] || die "no checksum entry for ${asset} in SHA256SUMS"
  [[ "$asset_sum" == "$expected" ]] \
    || die "checksum mismatch for ${asset}: got ${asset_sum}, expected ${expected}"
  echo "tcui: checksum verified (${asset_sum})"
else
  err "warning: SHA256SUMS unavailable at v${tag}; installed without checksum verification"
fi

# --- install -----------------------------------------------------------------
install_deb() {
  local file="$1"
  if (( EUID == 0 )); then
    dpkg -i "$file"
  else
    sudo dpkg -i "$file"
  fi
  echo "tcui: /usr/bin/tcui installed from .deb (re-run with TCUI_PKG=tarball to install without root)"
}

install_rpm() {
  local file="$1"
  if     command -v dnf    >/dev/null 2>&1; then sudo dnf    -y install "$file"
  elif   command -v yum    >/dev/null 2>&1; then sudo yum    -y install "$file"
  elif   command -v zypper >/dev/null 2>&1; then sudo zypper -y install "$file"
  else   die "no rpm frontend (dnf/yum/zypper) found"
  fi
  echo "tcui: /usr/bin/tcui installed from .rpm (re-run with TCUI_PKG=tarball to install without root)"
}

install_tarball() {
  local file="$1"
  local bin_dir="${TCUI_BIN_DIR:-}"
  [[ -n "$bin_dir" ]] || bin_dir="$([ "$EUID" = 0 ] && echo /usr/local/bin || echo "$HOME/.local/bin")"
  mkdir -p "$bin_dir"
  tar -xzf "$file" -C "$tmp"
  install -m 755 "${tmp}/tcui" "${bin_dir}/tcui"
  echo "tcui: installed to ${bin_dir}/tcui"
  case ":${PATH}:" in
    *":${bin_dir}:"*) ;;
    *) err "note: add ${bin_dir} to your PATH (or move tcui somewhere on it)" ;;
  esac
}

case "$pkg_type" in
  deb)     install_deb     "${tmp}/${asset}" ;;
  rpm)     install_rpm     "${tmp}/${asset}" ;;
  tarball) install_tarball "${tmp}/${asset}" ;;
esac