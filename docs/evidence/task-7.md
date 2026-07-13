# Todo7 OAuth cancellation dominance

## Invariants

- On Unix, `src/auth_command/runner.rs` registers SIGINT before starting the
  flow, then uses a biased outer `tokio::select!` with `sigint.recv()` before
  the flow branch. A selected signal latches cancellation, waits only for
  bounded teardown, discards every flow result, and returns
  `AuthCommandError::Cancelled`.
- `OAuthCancellation::is_cancelled` exposes the latched `watch` value.
  Headless input gives cancellation priority and checks the latch again after a
  successful read before it parses the value.
- OpenRouter checks the latch immediately before `KeyStore::upsert_credential`.
  A cancellation after a completed exchange therefore preserves the previous
  credential.

## Red and green tests

- Red: `rtk cargo test --locked --bin tcui
  signal_selected_simultaneously_with_transport_discards_transport_result`
  failed because `cancelled_after_teardown` and `persist_exchanged_key` did not
  exist.
- Green: `rtk cargo test --locked --bin tcui` passed `518` tests with `2`
  ignored.
- `rtk cargo test --locked --test auth_cli` passed all `8` integration tests.
- The SIGINT stress test uses `50` iterations, each cancelling both a Codex
  child process and a headless OpenRouter process: `100` SIGINT cases per run.
  It passed under the normal scheduler and under `--test-threads=1`.
- The stress fixture retains piped stdin while reaping the process so the test
  does not introduce an EOF/transport race. It verifies a killed process-group
  child, unauthenticated OpenRouter status, removed owned temporary roots, and
  no `/tmp/tcui-auth-cli-*` residue.

## Final verification

- Dev and release `cargo build --locked` passed.
- `rtk cargo clippy --locked --release -- -D warnings` passed.
- `target/release/tcui auth --help` ran successfully.
- Direct `rustfmt --edition 2024 --check` passed every Todo7 Rust source and
  test file. The full Cargo formatter command cannot run in this environment
  because its wrapper supplies `--edition` twice (`Option 'edition' given more
  than once`).
- LSP diagnostics report zero errors in all changed Rust files (only the
  expected inactive non-Unix hint in `runner.rs`). `git diff --check` passed.
- Full `rtk cargo test --locked` has `516` passing and `2` ignored; only the
  unrelated `memory::paths::paths_reject_absolute_parent_and_non_markdown_targets`
  and `memory::paths::paths_reject_symlink_escape` fixtures fail with
  `PermissionDenied`.
