#!/usr/bin/env bash
set -euo pipefail

cargo check --all-targets
cargo test
cargo run </dev/null >/tmp/tcui-smoke.out
test -s /tmp/tcui-smoke.out
