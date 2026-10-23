#!/usr/bin/env bash
# Package a glibc 2.34 x86-64 Linux release. Stable X.Y.Z versions only.
set -euo pipefail

if (( $# != 0 )); then
    echo 'Usage: bash scripts/package.sh' >&2
    exit 2
fi
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 - "$root" <<'PY'
import gzip
import hashlib
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import tomllib


def fail(message):
    raise RuntimeError(message)


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def git(*args):
    return run("git", *args, capture_output=True, text=True).stdout.strip()


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def check_output(path):
    if path.is_symlink() or (path.exists() and not path.is_dir()):
        fail(f"output must be a directory, not a file or symlink: {path}")
    if path.exists() and any(path.iterdir()):
        fail(f"output directory is not empty: {path}; move existing artifacts first")


def package():
    root = Path(sys.argv[1])
    os.chdir(root)
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        fail("packaging requires native x86-64 Linux")
    with Path("Cargo.toml").open("rb") as source:
        metadata = tomllib.load(source)["package"]
    version = metadata["version"]
    if not isinstance(version, str) or not re.fullmatch(
        r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version
    ):
        fail("release version must be stable SemVer X.Y.Z, without prerelease or build suffixes")
    repository = metadata.get("repository", "")
    if not re.fullmatch(r"https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        fail("package.repository must be an HTTPS GitHub repository URL without a trailing slash")
    if git("status", "--porcelain", "--untracked-files=all"):
        fail("release packaging requires a clean worktree, including untracked files")
    commit = git("rev-parse", "--verify", "HEAD^{commit}")
    timestamp = int(git("show", "-s", "--format=%ct", commit))
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target")).resolve()
    binary = target / "x86_64-unknown-linux-gnu/release/tcui"
    if binary.is_symlink() or not binary.is_file() or not os.access(binary, os.X_OK):
        fail(f"missing executable {binary}; run bash scripts/build-release.sh first")
    run("python3", "scripts/check-release-abi.py", str(binary))
    for path in ("LICENSE", "assets/models/potion-base-8M/LICENSE", "scripts/install.sh", "packaging/PKGBUILD"):
        if not Path(path).is_file():
            fail(f"missing packaging input: {path}")
    output = target / "dist"
    check_output(output)
    # Only our temporary directory is cleaned on failure. Existing outputs are never removed.
    with tempfile.TemporaryDirectory(prefix=".tcui-package-", dir=target) as temporary:
        assets = Path(temporary) / "dist"
        assets.mkdir(mode=0o755)
        environment = os.environ.copy()
        environment["SOURCE_DATE_EPOCH"] = str(timestamp)
        environment["CARGO_TARGET_DIR"] = str(target)
        run("cargo", "deb", "--locked", "--target", "x86_64-unknown-linux-gnu",
            "--no-build", "--no-strip", "--deb-revision", "1", "--output",
            str(assets / f"tcui_{version}_amd64.deb"), env=environment)
        # Host ldd-based auto-requires promotes optional weak versions to hard deps.
        # The ABI gate checks every required library and symbol before packaging.
        run("cargo", "generate-rpm", "--target", "x86_64-unknown-linux-gnu",
            "--target-dir", str(target), "--auto-req", "disabled", "--set-metadata", 'release = "1"',
            "--output", str(assets / f"tcui-{version}-1.x86_64.rpm"), env=environment)
        for source, name in (
            ("LICENSE", "LICENSE"),
            ("assets/models/potion-base-8M/LICENSE", "potion-base-8M-LICENSE"),
            ("scripts/install.sh", "install.sh"),
        ):
            shutil.copyfile(source, assets / name)
            (assets / name).chmod(0o755 if name == "install.sh" else 0o644)
        tarball = assets / "tcui-x86_64-unknown-linux-gnu.tar.gz"
        with tarball.open("wb") as raw:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=timestamp) as compressed:
                with tarfile.open(fileobj=compressed, mode="w") as archive:
                    for source, name in (
                        (binary, "tcui"),
                        (assets / "LICENSE", "LICENSE"),
                        (assets / "potion-base-8M-LICENSE", "potion-base-8M-LICENSE"),
                        (assets / "install.sh", "install.sh"),
                    ):
                        entry = archive.gettarinfo(str(source), arcname=name)
                        entry.uid = entry.gid = 0
                        entry.uname = entry.gname = "root"
                        entry.mtime = timestamp
                        entry.mode = 0o755 if name in ("tcui", "install.sh") else 0o644
                        with source.open("rb") as contents:
                            archive.addfile(entry, contents)
        # git archive embeds the exact candidate commit in its global pax header.
        run("git", "archive", "--format=tar.gz", f"--prefix=tcui-{version}/",
            f"--output={assets / f'tcui-{version}-source.tar.gz'}", commit)
        recipe = Path("packaging/PKGBUILD").read_text()
        for token, value in {
            "@VERSION@": version,
            "@REPOSITORY@": repository,
            "@TARBALL_SHA256@": sha256(tarball),
            "@LICENSE_SHA256@": sha256(assets / "LICENSE"),
            "@MODEL_LICENSE_SHA256@": sha256(assets / "potion-base-8M-LICENSE"),
        }.items():
            if token not in recipe:
                fail(f"missing PKGBUILD template token: {token}")
            recipe = recipe.replace(token, value)
        if re.search(r"@[A-Z_]+@", recipe):
            fail("unresolved PKGBUILD template token")
        (assets / "PKGBUILD").write_text(recipe)
        expected = {
            f"tcui_{version}_amd64.deb", f"tcui-{version}-1.x86_64.rpm",
            tarball.name, f"tcui-{version}-source.tar.gz", "PKGBUILD",
            "install.sh", "LICENSE", "potion-base-8M-LICENSE",
        }
        if {path.name for path in assets.iterdir()} != expected:
            fail("unexpected or missing release assets")
        for name in expected:
            path = assets / name
            if not stat.S_ISREG(path.lstat().st_mode) or not path.stat().st_size:
                fail(f"empty or non-regular release asset: {name}")
        (assets / "SHA256SUMS").write_text("".join(
            f"{sha256(assets / name)}  {name}\n" for name in sorted(expected)
        ))
        check_output(output)
        os.replace(assets, output)
    print(f"Packaged tcui {version} from {commit} in {output}")


try:
    package()
except (OSError, RuntimeError, ValueError, KeyError, subprocess.CalledProcessError) as error:
    print(f"package: {error}", file=sys.stderr)
    sys.exit(1)
PY
