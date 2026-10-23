#!/usr/bin/env bash
# Print only the validated commit; callers must build this SHA, not the workflow ref.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
tag="${1:?usage: release-ref.sh vX.Y.Z}"
[[ "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || {
  echo 'Release tags must be stable vX.Y.Z versions.' >&2; exit 1;
}
sha="$(git rev-parse --verify "refs/tags/$tag^{commit}")"
git merge-base --is-ancestor "$sha" refs/remotes/origin/main || {
  echo 'Release tag is not on main.' >&2; exit 1;
}
version="$(git show "$sha:Cargo.toml" | python3 -c 'import sys,tomllib; print(tomllib.loads(sys.stdin.read())["package"]["version"])')"
[[ "$tag" == "v$version" ]] || { echo 'Release tag does not match Cargo.toml.' >&2; exit 1; }
printf '%s\n' "$sha"
