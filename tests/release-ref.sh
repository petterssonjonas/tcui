#!/usr/bin/env bash
# Exercise the release trust boundary without a remote or application build.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/scripts"
cp scripts/release-ref.sh "$work/scripts/"
git init --quiet "$work"
git -C "$work" config user.email fixture@example.invalid
git -C "$work" config user.name 'Release fixture'
printf '[package]\nversion = "1.2.3"\n' > "$work/Cargo.toml"
git -C "$work" add Cargo.toml
git -C "$work" commit --quiet -m initial
sha="$(git -C "$work" rev-parse HEAD)"
git -C "$work" update-ref refs/remotes/origin/main "$sha"
git -C "$work" tag v1.2.3
expect_failure() {
  if bash "$work/scripts/release-ref.sh" "$@" > "$work/result" 2>&1; then
    echo 'Release guard accepted an invalid candidate.' >&2
    exit 1
  fi
}
test "$(bash "$work/scripts/release-ref.sh" v1.2.3 "$sha")" = "$sha"
expect_failure 'v1.2.3;false' "$sha"
expect_failure v01.2.3 "$sha"
expect_failure v1.2.3 main
expect_failure v1.2.3 0000000000000000000000000000000000000000
git -C "$work" tag v1.2.4
expect_failure v1.2.4 "$sha"
printf '\n# side branch commit\n' >> "$work/Cargo.toml"
git -C "$work" commit --quiet -am side
side="$(git -C "$work" rev-parse HEAD)"
git -C "$work" tag --force v1.2.3 "$side" > /dev/null
expect_failure v1.2.3 "$sha"
expect_failure v1.2.3 "$side"
printf 'Release identity, main ancestry, version, and input guards passed.\n'
