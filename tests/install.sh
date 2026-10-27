#!/usr/bin/env bash
# Focused installer regressions. Run from any directory: bash tests/install.sh
# Transport fixtures supply real tarballs and checksums; no network or sudo.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
[[ "$(uname -s)/$(uname -m)" == Linux/x86_64 ]] || { printf 'installer regressions require Linux/x86_64\n' >&2; exit 1; }
tmp="$(mktemp -d "${TMPDIR:-/tmp}/tcui-installer-test.XXXXXXXX")"
trap 'rm -rf -- "$tmp"' EXIT
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

export HOME="$tmp/home with spaces"
export TCUI_BIN_DIR="$HOME/.local/bin with spaces"
export TCUI_REPO=fixture/tcui
export TCUI_VERSION=v1.2.3
unset TCUI_RELEASE_TAG
export TMPDIR="$tmp/work"
mkdir -p -- "$TMPDIR"
export FIXTURE_DIR="$tmp/release"
export FIXTURE_BASE='https://github.com/fixture/tcui/releases/download/v1.2.3'
export FIXTURE_REQUESTS="$tmp/requests"
export REAL_UNAME
REAL_UNAME="$(type -P uname)"
mkdir -p -- "$HOME" "$TCUI_BIN_DIR" "$FIXTURE_DIR" "$tmp/transport" "$tmp/network" "$tmp/offline release"
cp -- "$root/scripts/install.sh" "$tmp/network/install.sh"
cp -- "$root/scripts/install.sh" "$tmp/offline release/install.sh"
fixture_binary="$(type -P true)"
cp -- "$fixture_binary" "$tmp/offline release/tcui"
# The old binary intentionally differs from the replacement; compare bytes, not
# command output, so merely reporting success cannot satisfy these assertions.
printf 'previous installation\n' > "$tmp/previous"

mkdir -p -- "$HOME/.config/tcui" "$HOME/.local/share/tcui/chats"
printf 'configuration\n' > "$HOME/.config/tcui/config.toml"
printf 'chat history\n' > "$HOME/.local/share/tcui/chats/history.json"
printf 'credentials\n' > "$HOME/.local/share/tcui/credentials"
cp -- "$HOME/.config/tcui/config.toml" "$tmp/config.original"
cp -- "$HOME/.local/share/tcui/chats/history.json" "$tmp/chats.original"
cp -- "$HOME/.local/share/tcui/credentials" "$tmp/credentials.original"

# Only curl's transport is replaced. Archive parsing, checksum verification,
# binary validation and filesystem changes all use the production installer.
cat > "$tmp/transport/curl" <<'CURL'
#!/usr/bin/env bash
set -euo pipefail
url=''
out=''
while (( $# )); do
    case "$1" in
        https://*) url="$1"; shift ;;
        --output) out="$2"; shift 2 ;;
        *) shift ;;
    esac
done
[[ -n "$out" && "$url" == "$FIXTURE_BASE/"* ]] || exit 22
name="${url#"$FIXTURE_BASE/"}"
case "$name" in tcui-x86_64-unknown-linux-gnu.tar.gz|SHA256SUMS) ;; *) exit 22 ;; esac
printf '%s\n' "$url" >> "$FIXTURE_REQUESTS"
[[ -f "$FIXTURE_DIR/$name" ]] || exit 22
cp -- "$FIXTURE_DIR/$name" "$out"
CURL
chmod +x -- "$tmp/transport/curl"
export PATH="$tmp/transport:$PATH"

asset='tcui-x86_64-unknown-linux-gnu.tar.gz'
payload="$tmp/payload"
mkdir -p -- "$payload"
cp -- "$fixture_binary" "$payload/tcui"
cp -- "$root/scripts/install.sh" "$payload/install.sh"
printf 'fixture project license\n' > "$payload/LICENSE"
printf 'fixture font license\n' > "$payload/potion-base-8M-LICENSE"

checksums() { (cd -- "$FIXTURE_DIR" && sha256sum -- "$asset" > SHA256SUMS); }
archive() {
    tar -czf "$FIXTURE_DIR/$asset" -C "$payload" tcui install.sh LICENSE potion-base-8M-LICENSE
    checksums
}
assert_data() {
    cmp -s -- "$tmp/config.original" "$HOME/.config/tcui/config.toml" || fail 'configuration changed'
    cmp -s -- "$tmp/chats.original" "$HOME/.local/share/tcui/chats/history.json" || fail 'chats changed'
    cmp -s -- "$tmp/credentials.original" "$HOME/.local/share/tcui/credentials" || fail 'credentials changed'
}
assert_clean() {
    if compgen -G "$TCUI_BIN_DIR/.tcui-install.*" > /dev/null ||
        compgen -G "$TMPDIR/tcui-install.*" > /dev/null; then
        fail 'installer left a staging or download directory'
    fi
}
reset_install() { cp -- "$tmp/previous" "$TCUI_BIN_DIR/tcui"; }
expect_failure() {
    local label="$1"
    shift
    if "$@" > "$tmp/output" 2>&1; then
        fail "$label unexpectedly succeeded"
    fi
    cmp -s -- "$tmp/previous" "$TCUI_BIN_DIR/tcui" || fail "$label changed existing installation"
    assert_data
    assert_clean
}
network_install() { bash "$tmp/network/install.sh"; }
offline_install() { bash "$tmp/offline release/install.sh" "$@"; }
embedded_install() { bash -c "$(< "$root/scripts/install.sh")" tcui-updater; }

# Offline install uses a sibling binary, tolerates spaces and does not invoke curl.
reset_install
offline_install > "$tmp/output" 2>&1
cmp -s -- "$fixture_binary" "$TCUI_BIN_DIR/tcui" || fail 'offline binary differs'
[[ -x "$TCUI_BIN_DIR/tcui" && ! -e "$FIXTURE_REQUESTS" ]] || fail 'offline install permissions or transport'
assert_data
offline_install uninstall > "$tmp/output" 2>&1
[[ ! -e "$TCUI_BIN_DIR/tcui" && -d "$TCUI_BIN_DIR" ]] || fail 'uninstall removed wrong path'
assert_data
offline_install uninstall > "$tmp/output" 2>&1
assert_data

# Default destination never changes to /usr/local/bin, including when run as root.
env -u TCUI_BIN_DIR bash "$tmp/offline release/install.sh" > "$tmp/output" 2>&1
cmp -s -- "$fixture_binary" "$HOME/.local/bin/tcui" || fail 'default destination differs'
env -u TCUI_BIN_DIR bash "$tmp/offline release/install.sh" uninstall > "$tmp/output" 2>&1
[[ ! -e "$HOME/.local/bin/tcui" ]] || fail 'default uninstall failed'

# Explicit and latest release routes both install real, checksum-verified bytes.
archive
reset_install
network_install > "$tmp/output" 2>&1
cmp -s -- "$fixture_binary" "$TCUI_BIN_DIR/tcui" || fail 'verified release did not install'
TCUI_VERSION=1.2.3 network_install > "$tmp/output" 2>&1
TCUI_VERSION=latest FIXTURE_BASE='https://github.com/fixture/tcui/releases/latest/download' network_install > "$tmp/output" 2>&1
cmp -s -- "$fixture_binary" "$TCUI_BIN_DIR/tcui" || fail 'latest release did not install'
assert_data

# Exact release pinning preserves the real tag. It takes priority over the
# compatibility TCUI_VERSION setting, including in the embedded updater route.
reset_install
TCUI_RELEASE_TAG=0.8.6 TCUI_VERSION=v0.8.5 FIXTURE_BASE='https://github.com/fixture/tcui/releases/download/0.8.6' embedded_install > "$tmp/output" 2>&1
cmp -s -- "$fixture_binary" "$TCUI_BIN_DIR/tcui" || fail 'unprefixed exact release did not install'
TCUI_RELEASE_TAG=v0.8.6 FIXTURE_BASE='https://github.com/fixture/tcui/releases/download/v0.8.6' network_install > "$tmp/output" 2>&1
cmp -s -- "$fixture_binary" "$TCUI_BIN_DIR/tcui" || fail 'prefixed exact release did not install'
assert_clean
reset_install
expect_failure 'invalid exact release tag' env TCUI_RELEASE_TAG='0.8.6/../../other' bash "$tmp/network/install.sh"

# Missing, incomplete, malformed, duplicate and incorrect checksum manifests fail
# before any replacement. A tarball with no SHA256SUMS must never be accepted.
reset_install
rm -- "$FIXTURE_DIR/SHA256SUMS"
expect_failure 'missing checksum manifest' network_install
printf '%064d  unrelated.tar.gz\n' 0 > "$FIXTURE_DIR/SHA256SUMS"
expect_failure 'missing asset checksum' network_install
printf 'not-a-digest  %s\n' "$asset" > "$FIXTURE_DIR/SHA256SUMS"
expect_failure 'malformed checksum' network_install
checksums
cat -- "$FIXTURE_DIR/SHA256SUMS" > "$tmp/sums"
cat -- "$tmp/sums" "$tmp/sums" > "$FIXTURE_DIR/SHA256SUMS"
expect_failure 'duplicate checksum' network_install
printf '%064d  %s\n' 0 "$asset" > "$FIXTURE_DIR/SHA256SUMS"
expect_failure 'incorrect checksum' network_install

# Correct checksums do not make an unsafe archive acceptable.
printf 'not a tarball\n' > "$FIXTURE_DIR/$asset"
checksums
expect_failure 'corrupt archive' network_install
tar -czf "$FIXTURE_DIR/$asset" -C "$payload" tcui install.sh LICENSE potion-base-8M-LICENSE tcui
checksums
expect_failure 'duplicate binary archive entry' network_install
printf 'outside data\n' > "$tmp/outside"
tar -czf "$FIXTURE_DIR/$asset" -C "$payload" tcui install.sh LICENSE potion-base-8M-LICENSE -C "$tmp" --transform='s|^outside$|../outside|' outside
checksums
expect_failure 'archive traversal' network_install
[[ "$(cat -- "$tmp/outside")" == 'outside data' ]] || fail 'archive escaped staging'
rm -- "$payload/tcui"
ln -s -- "$fixture_binary" "$payload/tcui"
archive
expect_failure 'symlink binary archive entry' network_install
rm -- "$payload/tcui"
mkdir -- "$payload/tcui"
archive
expect_failure 'directory binary archive entry' network_install
rmdir -- "$payload/tcui"
printf '#!/bin/sh\ntouch "%s"\n' "$tmp/executed" > "$payload/tcui"
archive
expect_failure 'non-ELF archive binary' network_install
[[ ! -e "$tmp/executed" ]] || fail 'downloaded binary was executed'
cp -- "$payload/tcui" "$tmp/offline release/tcui"
expect_failure 'non-ELF offline binary' offline_install
[[ ! -e "$tmp/executed" ]] || fail 'offline binary was executed'
cp -- "$fixture_binary" "$tmp/offline release/tcui"
printf '\267\000' | dd of="$tmp/offline release/tcui" bs=1 seek=18 conv=notrunc status=none
expect_failure 'wrong-architecture offline binary' offline_install
cp -- "$fixture_binary" "$tmp/offline release/tcui"

# Failed copy and rename leave the installed bytes and user data unchanged, and
# remove both private download storage and destination-filesystem staging.
cp -- "$fixture_binary" "$payload/tcui"
archive
reset_install
for program in install mv; do
    printf '#!/usr/bin/env bash\nexit 1\n' > "$tmp/transport/$program"
    chmod +x -- "$tmp/transport/$program"
    expect_failure "failed $program" embedded_install
    rm -- "$tmp/transport/$program"
done

# Refuse symlinks instead of following them, and never recursively remove targets.
rm -- "$TCUI_BIN_DIR/tcui"
ln -s -- "$tmp/previous" "$TCUI_BIN_DIR/tcui"
for action in install uninstall; do
    if offline_install "$action" > "$tmp/output" 2>&1; then fail "symlink target $action succeeded"; fi
    [[ -L "$TCUI_BIN_DIR/tcui" ]] || fail 'symlink target was replaced'
    [[ "$(cat -- "$tmp/previous")" == 'previous installation' ]] || fail 'symlink referent changed'
done
rm -- "$TCUI_BIN_DIR/tcui"
mkdir -- "$TCUI_BIN_DIR/tcui"
printf 'keep\n' > "$TCUI_BIN_DIR/tcui/user-data"
if offline_install uninstall > "$tmp/output" 2>&1; then fail 'directory uninstall succeeded'; fi
[[ -f "$TCUI_BIN_DIR/tcui/user-data" ]] || fail 'directory target was removed'
rm -- "$TCUI_BIN_DIR/tcui/user-data"
rmdir -- "$TCUI_BIN_DIR/tcui"
reset_install
ln -s -- "$TCUI_BIN_DIR" "$tmp/linked directory"
for action in install uninstall; do
    expect_failure "symlink parent $action" env TCUI_BIN_DIR="$tmp/linked directory" bash "$tmp/offline release/install.sh" "$action"
    for bad_path in / relative "$TCUI_BIN_DIR/../bin with spaces" "$TCUI_BIN_DIR/" "$TCUI_BIN_DIR//nested" ''; do
        expect_failure "unsafe destination $action" env TCUI_BIN_DIR="$bad_path" bash "$tmp/offline release/install.sh" "$action"
    done
done
rm -- "$tmp/offline release/tcui"
ln -s -- "$fixture_binary" "$tmp/offline release/tcui"
expect_failure 'symlink offline source' offline_install
rm -- "$tmp/offline release/tcui"
cp -- "$fixture_binary" "$tmp/offline release/tcui"

# Simulated unsupported platform must fail before changing the installation.
cat > "$tmp/transport/uname" <<'UNAME'
#!/usr/bin/env bash
if [[ "$1" == -m ]]; then printf 'aarch64\n'; else exec "$REAL_UNAME" "$@"; fi
UNAME
chmod +x -- "$tmp/transport/uname"
expect_failure 'unsupported architecture' offline_install
rm -- "$tmp/transport/uname"
assert_data
printf 'Installer regressions passed.\n'
