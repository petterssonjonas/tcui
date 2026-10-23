#!/usr/bin/env bash
# Install a Linux x86-64 release into ~/.local/bin without sudo or package managers.
# Usage: bash install.sh [install|uninstall]
# TCUI_BIN_DIR selects an absolute destination; TCUI_REPO selects owner/repository.
# TCUI_VERSION selects a release tag (with or without v), defaulting to latest.
# A regular sibling tcui is a trusted, already-extracted offline release. Otherwise
# the downloaded tarball must match its release SHA256SUMS before it is opened.
set -euo pipefail

err() { printf 'tcui: %s\n' "$*" >&2; }
die() { err "$*"; exit 1; }

(( $# <= 1 )) || die 'usage: bash install.sh [install|uninstall]'
action="${1:-install}"
case "$action" in install|uninstall) ;; *) die 'usage: bash install.sh [install|uninstall]' ;; esac

bin_dir="${TCUI_BIN_DIR-${HOME:?HOME must be set}/.local/bin}"
[[ "$bin_dir" == /* && "$bin_dir" != / ]] || die 'TCUI_BIN_DIR must be an absolute directory other than /'
[[ "$bin_dir" != *$'\n'* && "$bin_dir" != *$'\r'* ]] || die 'TCUI_BIN_DIR must not contain newlines'
# Reject ambiguous paths and symlink components rather than following them during
# installation or removal. Spaces are ordinary path characters.
IFS=/ read -r -a components <<< "${bin_dir#/}"
path=''
for component in "${components[@]}"; do
    [[ -n "$component" && "$component" != . && "$component" != .. ]] || die 'TCUI_BIN_DIR must not contain empty, . or .. components'
    path="$path/$component"
    [[ ! -L "$path" ]] || die "refusing symlink directory: $path"
    if [[ -e "$path" ]]; then
        [[ -d "$path" ]] || die "not a directory: $path"
    fi
done
[[ "$bin_dir" != */ ]] || die 'TCUI_BIN_DIR must not end with /'
target="$bin_dir/tcui"
[[ ! -L "$target" ]] || die "refusing symlink target: $target"
if [[ -e "$target" ]]; then
    [[ -f "$target" && -O "$target" ]] || die "target must be a regular file owned by the current user: $target"
fi

if [[ "$action" == uninstall ]]; then
    # Never remove directories, configuration, chats, credentials or symlink targets.
    if [[ -f "$target" ]]; then
        rm -- "$target"
        printf 'tcui: removed %s; user data preserved\n' "$target"
    else
        printf 'tcui: no installation at %s; user data preserved\n' "$target"
    fi
    exit 0
fi

[[ "$(uname -s)" == Linux ]] || die 'installer supports Linux only'
case "$(uname -m)" in x86_64|amd64) ;; *) die 'installer supports x86-64 only' ;; esac

work=''
stage=''
cleanup() {
    [[ -z "$stage" ]] || rm -rf -- "$stage"
    [[ -z "$work" ]] || rm -rf -- "$work"
    return 0
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

source_binary=''
script="${BASH_SOURCE[0]:-}"
if [[ -n "$script" && -f "$script" ]]; then
    script_dir="$(cd -- "$(dirname -- "$script")" && pwd -P)"
    if [[ -e "$script_dir/tcui" || -L "$script_dir/tcui" ]]; then
        [[ -f "$script_dir/tcui" && ! -L "$script_dir/tcui" ]] || die 'offline tcui must be a regular file, not a symlink'
        source_binary="$script_dir/tcui"
    fi
fi

if [[ -z "$source_binary" ]]; then
    repo="${TCUI_REPO:-petterssonjonas/tcui}"
    version="${TCUI_VERSION:-latest}"
    [[ "$repo" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || die 'TCUI_REPO must be owner/repository'
    if [[ "$version" == latest ]]; then
        base="https://github.com/$repo/releases/latest/download"
    else
        version="${version#v}"
        [[ "$version" =~ ^[0-9][A-Za-z0-9.+-]*$ ]] || die 'TCUI_VERSION must be a version such as v1.2.3 or latest'
        base="https://github.com/$repo/releases/download/v$version"
    fi
    asset='tcui-x86_64-unknown-linux-gnu.tar.gz'
    work="$(mktemp -d "${TMPDIR:-/tmp}/tcui-install.XXXXXXXX")"
    for name in "$asset" SHA256SUMS; do
        curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
            "$base/$name" --output "$work/$name" || die "could not download $name"
    done

    expected=''
    while read -r digest filename extra; do
        if [[ "$filename" == "$asset" || "$filename" == "*$asset" ]]; then
            [[ -z "$expected" && -z "$extra" && "$digest" =~ ^[[:xdigit:]]{64}$ ]] || die "invalid or duplicate checksum for $asset"
            expected="${digest,,}"
        fi
    done < "$work/SHA256SUMS"
    [[ -n "$expected" ]] || die "missing checksum for $asset"
    actual="$(sha256sum -- "$work/$asset")"
    [[ "${actual%% *}" == "$expected" ]] || die "checksum mismatch for $asset"

    # Accept only the four regular root files in the release contract. Never
    # extract paths from the archive: stream only tcui into our private directory.
    listing="$(tar --list --gzip --file "$work/$asset")" || die 'invalid release archive'
    declare -A members=()
    while IFS= read -r member; do
        case "$member" in tcui|install.sh|LICENSE|potion-base-8M-LICENSE) ;; *) die "unexpected archive member: $member" ;; esac
        [[ ! -v "members[$member]" ]] || die "duplicate archive member: $member"
        members["$member"]=1
    done <<< "$listing"
    (( ${#members[@]} == 4 )) || die 'release archive is missing required files'
    details="$(tar --list --verbose --gzip --file "$work/$asset")" || die 'invalid release archive'
    while IFS= read -r detail; do
        [[ "$detail" == -* ]] || die 'release archive members must be regular files'
    done <<< "$details"
    tar --extract --gzip --to-stdout --file "$work/$asset" -- tcui > "$work/tcui" || die 'could not read release binary'
    source_binary="$work/tcui"
fi

# Check the ELF header without running an untrusted binary. Require little-endian
# ELF64, executable or shared-object type, and the AMD64 machine identifier.
header="$(od -An -tu1 -N20 -- "$source_binary")"
read -r -a bytes <<< "${header//$'\n'/ }"
[[ ${#bytes[@]} == 20 ]] || die 'release binary has an invalid ELF header'
[[ ${bytes[0]} == 127 && ${bytes[1]} == 69 && ${bytes[2]} == 76 && ${bytes[3]} == 70 &&
   ${bytes[4]} == 2 && ${bytes[5]} == 1 && ${bytes[6]} == 1 &&
   ( ${bytes[16]} == 2 || ${bytes[16]} == 3 ) && ${bytes[17]} == 0 &&
   ${bytes[18]} == 62 && ${bytes[19]} == 0 ]] || die 'release binary must be an x86-64 Linux ELF executable'

# Stage on the destination filesystem, then rename atomically. A failed download,
# validation or copy leaves the previous installation untouched.
mkdir -p -- "$bin_dir"
stage="$(mktemp -d "$bin_dir/.tcui-install.XXXXXXXX")"
install -m 755 -- "$source_binary" "$stage/tcui"
mv -fT -- "$stage/tcui" "$target"
printf 'tcui: installed %s\n' "$target"
case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *) err "add $bin_dir to PATH to run tcui" ;;
esac
