# TCUI actionable follow-up review — 2026-10-22

## Scope and conclusion

This reviews [Fable's report](2026-10-21-review.md) against the **current working tree**, not just its stated baseline, `de4c099`. HEAD is `2234c71`; `git rev-list --count de4c099..HEAD` reports **40 commits**. The working tree also contains ongoing terminal, command-block, Markdown, dependency, and documentation changes, including the new `markdown_shell.rs`. Those changes were included where relevant and left untouched.

The earlier report is useful, but its work batches should not be executed verbatim. The settings workspace, credential backend, native Vault operations, search implementation, and notification handling have changed substantially. Several proposed fixes now duplicate working implementations or would remove live functionality.

**Current priorities:** request-owned streaming is implemented under AR-01. The tag-safety follow-up closes the memory authorization bypass (AR-08), removes hidden title/memory commands (AR-13), and fixes Unicode title truncation in AR-04. Contextual memory and scoped metadata are now implemented under AR-15. Next, prevent destructive config rewrites and fix the remaining crash/terminal-cleanup paths. CI/CD is owned by a separate session (AR-14). Keep the current memory backend unless the owner chooses a replacement. App-wide password protection is planned under AR-17.

Evidence labels used below:

- **Observed:** exercised in a local runtime probe described in §1.
- **Source:** current source establishes the implementation; the complete user scenario was not executed.
- **[INFERENCE]:** consequence or exploitability inferred from that implementation, not demonstrated end to end.

This is a targeted reassessment, not a fresh exhaustive security audit. The initial assessment did not change production code or tests. The AR-01, AR-04, AR-08, AR-13, and AR-15 updates record subsequent requested implementation work. Other findings remain open unless stated otherwise. The SQLite credential vault and the complete memory system have not received a fresh exhaustive security audit.

## 1. Verification performed

### Initial assessment: build and dependency evidence

- `cargo build --locked --all-features` passed against the current working tree.
- `cargo audit --json` could not run: `error: no such command: audit`. No new audit count is claimed.
- Read the locked versions and checked primary RustSec advisories; see AR-12.
- `cargo tree --locked --all-features -i rustls` confirms rustls is active through both reqwest versions.
- `cargo tree --locked --all-features -i crossbeam-epoch` confirms it is active through Rayon.
- `cargo tree --locked --all-features -i quinn-proto` reports nothing to print for this host/feature selection. A lockfile entry is not proof that vulnerable QUIC code runs in this build.
- Full tests, clippy, no-default-features builds, authenticated provider calls, and a full interactive UI session were **not** run. The earlier test counts and lint inventory are historical, not current verification.

### Initial assessment: isolated runtime probes

All fixtures used temporary HOME/XDG directories and were removed. No real credentials, notes, or user configuration were used.

1. **Unknown-field config loss.** Invoked the built binary as `tcui reminder-dispatch review-no-such-record`. That command loads config and returns without notification delivery when the isolated reminder store has no matching record. After generating canonical config, prepended a comment and an unknown root key. The next invocation exited 0 and removed both. A subsequent load was byte-identical. A separate canonical config with only an added comment remained byte-identical. Therefore, “every load deletes comments” is too broad; normalization/migration-triggered rewriting deletes them.
2. **Terminal cleanup on storage failure.** Launched the built binary on an isolated PTY with `data/tcui/tcui.db` deliberately created as a directory. It exited 1 at `src/storage/db.rs:42`. ICANON and ECHO remained off; the output entered the alternate screen but never left it. The harness restored and closed its own PTY afterward. Malformed config is no longer the right reproduction: config loading now precedes terminal setup.
3. **Unsafe config temporary file.** In an isolated config directory, planted `.config.toml.tmp` as a symlink to a synthetic sentinel, then triggered config normalization. The command exited 0, overwrote the outside sentinel, and replaced `config.toml` with the symlink. This requires control over the config directory; it is not a demonstrated remote attack.
4. **Unicode title panic.** Compiled the verbatim current `generate_title` function in a disposable Rust harness. `Привет мир как дела` and `日本語日本語日本語` panicked. Five crab emoji happened to truncate safely because byte 16 aligns with their encoding. This exercised the actual helper body, not an end-to-end chat send.
5. **Existing headless smoke coverage.** The built binary with non-TTY input/output exited 0 with `TermChatUI requires an interactive terminal.` It did not create config. The current CI smoke therefore does not exercise app startup, storage initialization, or rendering.

### Tag-safety follow-up verification

- `cargo build --locked --all-features` passed.
- `cargo test --locked --all-features -- --test-threads=1` passed: 1,190 unit tests and 13 integration tests; three tests remained ignored.
- `cargo test --locked --no-default-features -- --test-threads=1` passed: 1,157 unit tests and 13 integration tests; one live OAuth test remained ignored.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --locked --all-targets --all-features -- -D warnings` failed on the same 16 existing lints. These include test environment locks held across awaits, unnecessary returns, and module/test-layout issues. No lint suppression or unrelated cleanup was added; this is not a green lint/CI claim.
- Verification used the installed Fedora Rust 1.99.0 toolchain. The repository's 1.96.0 toolchain was not available through rustup or a remaining project-local installation, so this run does not certify the pinned CI toolchain.
- The default target cache contained an unwritable fingerprint. Checks used the existing owned alternate target cache without changing permissions, with isolated HOME/XDG directories and one test thread.
- A real PTY and a local, chunked SSE fixture verified literal user/assistant tags, Unicode titles, quoted directives with no implicit save, unfinished-answer preservation after restart, explicit memory save/read, 500/501-character boundaries, rejection when Memory is disabled, and durable explicit saves from temporary chats after close/restart. No live model account was used.
- Independent source reviews covered memory authorization and literal text/provider transport behavior. Findings about raw input limits, Unicode XML names, and empty SDK deltas were corrected. The pre-existing durable-write/index-status issue remains tracked under AR-06 and AR-15.

The terminal, local fixture server, and isolated data were removed. Existing user data, build-cache permissions, and CI/CD work were not changed by this session.

### Contextual-memory follow-up verification

Work is isolated in `/home/jp/Projects/.worktrees/tcui/contextual-memory`, branch `fix/contextual-memory`, based on `2234c71`. The worktree includes the pre-existing application and completed CI/release changes from `/home/jp/Projects/tcui`. The implementation checkpoint below preceded publication; its checks are local working-tree evidence.

Checks ran through `toolbox run --container tcui mise exec -- ...`, using the repository-pinned Rust **1.99.1**, isolated HOME/XDG directories, and the owned target cache `/home/jp/Projects/tcui/target/contextual-memory`.

| Configuration | Unit tests passed | Integration tests passed | Tests ignored |
|---|---:|---:|---:|
| `--all-features` | 1,214 | 13 | 3 |
| `--no-default-features` | 1,170 | 13 | 1 |

Both `cargo test --locked <configuration> -- --test-threads=1` runs passed. Both `cargo clippy --locked --all-targets <configuration> -- -D warnings` runs passed. `cargo build --locked --all-features` and `cargo fmt --all -- --check` passed. These are working-tree results, not CI results for a published candidate commit. The older 16-diagnostic inventory below is historical.

A real terminal, local SSE provider fixture, and separate memory MCP process exercised contextual save, recall without mutation, clarification resumed after restart, machine/source metadata, read-only rejection, malformed-plan rejection, and a completed save followed by final-answer provider failure. The viewer displayed persisted metadata. A terminal-discovered `/skill` Enter issue was fixed; the rebuilt application submitted and completed recall with exactly one Enter. Completed-save activity was still visible after restart.

Two independent source reviews identified invocation-boundary, duplicate-metadata, partial-outcome, provider-failure activity, and credential-guard issues. Those findings were corrected before the final checks; the reviewers did not approve a new final revision. The runtime provider was controlled, not a live language model. These checks prove the integration paths, not universal model intent accuracy.

The terminal, provider server, MCP process, and isolated data were closed or removed. The worktree and build cache remain for handoff.

### Publication and parallel-session boundary

The owner subsequently requested moving the registered worktree to `~/Projects/.worktrees/tcui/contextual-memory` and publishing all completed changes as a PR to `development`. The paused workflow-alignment session had made no source edits: its recorded actions were inspection, verification, and questions. The original checkout's tracked diff matched the retained implementation-start snapshot exactly. Existing CI/release changes came from the earlier completed session and belong to this accumulated baseline, not the pending alignment task.

The PR includes that completed baseline and the contextual-memory implementation. Runtime `log` and `tcui-chats` data are excluded. The original checkout is preserved without reset, cleanup, or staging. No merge or auto-merge is authorized.

For the paused session, the clean base is `origin/development` after this PR is reviewed and merged. Then create `~/Projects/.worktrees/tcui/workflow-alignment` from the refreshed branch and implement only the pending governance/local-test/CI alignment. Do not copy the original checkout's old uncommitted snapshot into that new worktree: those changes will already be in the merged baseline. Preserve the original checkout until its remaining differences and runtime data are reconciled deliberately.

## 2. Actionable findings

Priority meanings: **P1** = address before more feature expansion; **P2** = next hardening/correctness batch; **P3** = measured maintenance work.

### AR-01 — Bind every asynchronous chat event to its owning request

**P1 · Implemented follow-up · Source + Observed · CQ-3, CQ-12**

**Reproduced before the change:** two real terminal chats used a local, gated HTTP SSE fixture. A late completion from the first chat replaced the second chat's visible answer. The old vector-index route was therefore a demonstrated correctness defect, not only an inference.

**Implemented:** `RequestRoute` carries the existing `SubmissionId`, an immutable origin conversation ID, and a destination conversation ID. `TuiApp::requests` owns each response snapshot and abort handle. Content, thinking, title, memory activity, finalization, and stop events resolve through that registry. Inactive conversations retain their streams; the visible chat is only a view. One request is allowed per destination, with independent requests in other chats.

Finalization validates the request before file imports and persists its destination snapshot rather than the visible history. Missing persistent destinations are recreated from known context without changing the request or origin ID. A removed chat owner is rebound to a surviving workspace, with the recovered conversation visible in its sidebar. Corruption and permission failures remain errors, not permission to replace data. Unknown or completed request events cannot modify another request or create artifacts. Closing an ephemeral destination is an explicit discard: its request is removed, and it is never recovered as a persistent chat.

`/cancel` and double Escape within 1000 ms abort the local worker and retain partial text. Persistent chats save it to disk; ephemeral chats retain it only in memory. Escape remains owned by dialogs and focused terminals; view changes and intervening input reset the chord. Normal exit saves remaining persistent responses and discards the ephemeral chat. An unexpectedly ended worker marks its partial response interrupted. Cancellation cannot reverse completed side effects or guarantee cancellation of remote server work; request preparation is not yet independently cancellable.

**Changed recommendation:** the initial suggestion to cancel on navigation or persistent-chat deletion is withdrawn. Navigation changes the view, not request ownership. Persistent-chat deletion creates a recovery destination, as requested. Ephemeral close instead cancels and discards. Future task callers can supply different origin and destination IDs through the same registry; no model-task scheduler was added.

**Related behavior:** `/fork` creates and selects an independent encrypted history named `Fork of <title>`, including a live partial answer, without moving the parent's request. It retains model/provider selection and records optional lineage; Chat file links are not copied. Bounded local conversation recall supplies at most three source-labelled excerpts and 3,000 characters to the provider request only. It excludes current/trash/ephemeral/duplicate content, works without the Memory feature, and uses lexical matching with explicit scan limits. This is not an unlimited semantic-memory implementation.

**Steering limit:** `/steer <instruction>` is recognized, but current HTTP SSE and Claude print-mode connections do not support real live input. It reports that limit and preserves the request and draft. It never claims success or simulates steering by silently restarting the request.

#### Earlier routing implementation verification

**Runtime evidence:** the updated terminal kept B unchanged when A completed; refused a second B request without losing its draft; cancelled B with double Escape; created a sidebar fork; sent 424 characters of source-labelled A evidence in a later provider request; recovered a deleted live destination and continued its output; and restored the recovered answer after restart. Normal exit also saved an active response as cancelled. The smoke found a completion-popup interception issue with `/steer`; that was fixed and rechecked with one Enter.

The final all-feature binary build passed. Its in-app Help view was checked in a real PTY and showed `/fork`, `/cancel`, and `/steer`. The local fixture processes and isolated runtime data were removed after verification.

**Regression coverage:** ownership across chat switches, explicit different destinations, missing destinations at launch, removed owners, stale finalization/artifacts, cancellation, abort cleanup, corrupt storage, fork isolation/persistence/lineage, bounded recall, and composer/modal/terminal input boundaries.

**Verification:** the all-feature suite passed 1,158 unit tests and 13 integration tests, with three tests ignored. Its XDG integration runner requires isolated sibling HOME/config/data/cache directories; the final run supplied those. The no-default-feature binary suite passed all 1,120 tests. An existing root-owned fingerprint prevented that feature build in the normal target directory, so verification used a separate target directory without changing existing permissions. `cargo clippy --all-features --all-targets` completed with 16 warnings; it is not a warning-clean result. `cargo fmt --check` passed. Both suites ran with one test thread. Earlier failing runs exposed a history-shape assumption and a poisoned shared test lock; the relevant assertion was removed while content-preservation checks remained.

#### Temporary-fork follow-up

`/temp` now opens `Ephemeral fork of <title> conversation` in the Wide right sidebar. It reserves space rather than covering main. Both panes use the existing chat renderer and have separate input, documents, scrolling, and request ownership. Alt+Left/Alt+Right or a pointer action selects the pane. The left conversation list remains bound to main. Repeating `/temp` focuses the existing pane without discarding it.

Ephemeral documents use a clone-shared, memory-only store and negative IDs. They never enter encrypted chat files, trash, persistent listings, bulk exports, or automatic recall. Closing the pane drops its abort handle and removes its document and input state. Queued output cannot recreate it or save a late artifact. Normal exit uses the same discard path.

Explicit `@save` creates a standalone Saved file, without a persistent link to an ephemeral chat. Explicit memory operations still use the existing Memory system; implicit remember directives are rejected for ephemeral requests. Ordinary ephemeral answers also do not auto-import local media. These restrictions do not revoke explicit skill operations or erase their completed effects.

An explicit request in main to read the temporary chat copies visible branch messages into the request and saved main history. It includes live partial text, excludes hidden thinking, and labels the copy as untrusted reference data. The original source chat receives only new branch messages; another main conversation receives the full snapshot. Skill and memory-write intent are evaluated separately from that copy. An explicit copy can be large and survives temporary-chat deletion.

**Observed in a real terminal with an isolated local SSE fixture:** both panes rendered independently; main received the requested live branch text without duplicating inherited history; quoted `@save`/`@remember` markers did not create saved results; Escape presses 800 ms apart cancelled the temporary response; an unsolicited memory directive created no memory; explicit `@save` and `@remember` produced durable results; main retrieved that memory after close; the close button cancelled a streaming temporary chat while main continued; `/fork` selected `Fork of Origin`; normal exit discarded an active temporary fork. Restart restored the persistent fork and approved context copy, but no temporary pane. The active chat directory contained two persistent documents and no temporary documents. The explicitly saved file and memory remained.

**Checks:** the full all-feature test command and the no-default-feature binary suite passed, each with one test thread and isolated HOME/XDG directories. The no-default build reused the owned alternate target directory rather than changing permissions on the existing root-owned cache. Clippy completed with zero errors and the same 16 existing warnings. New regression coverage includes memory-only storage isolation, stale writes, worker abort, explicit artifact persistence, context-copy boundaries, pane ownership, input focus, modal priority, and Markdown control routing.


`cargo fmt --all -- --check` and the final all-feature build passed. The rebuilt terminal Help showed `/temp`, `/temp close`, the fork naming rule, and the 1000 ms cancellation window. The isolated PTY sessions, local SSE server, and their runtime directories were closed and removed after verification.

### AR-02 — Stop treating normalized serialization as a migration

**P1 · Observed + Source · SET-2**

`src/config/app_config/persistence.rs:51-73` compares deserialized/re-serialized TOML values and writes whenever they differ. Unknown fields and absent defaults can cause a rewrite without a real schema migration. The memory field remains feature-gated (`src/config/app_config.rs:289-290`), so [INFERENCE] a no-memory build still loses memory configuration when rewriting it.

**Fix:** distinguish real migrations from ordinary reads. Read an existing current-schema file without rewriting it. Keep explicit first-run creation and genuine Vault/profile migrations; do not delete those necessary operations merely to satisfy “load is pure.” Preserve unrecognized fields and comments during migrations and individual setting saves. Prefer editing the existing TOML document at the changed paths rather than serializing the entire typed config. Preserve inactive feature sections even when their runtime implementation is disabled.

**Better than the old proposal:** `#[serde(flatten)]` alone preserves neither comments nor unknown nested fields. Making a feature's type unconditional is one option, not a reason to compile its entire subsystem into every build. Separate configuration data from feature implementation, or preserve the untouched document section.

**Done when:** repeated ordinary loads preserve bytes; a real migration changes only intended fields; saving one setting preserves comments, unknown root/nested keys, and disabled-feature sections. Exercise both feature builds. Keep migration rollback and backup behavior.

### AR-03 — Harden the existing atomic writer instead of adding another one

**P2 · Observed + Source · SET-11, SEC-10**

Config persistence now has locking, sync, rename, and rollback (`src/config/app_config/persistence/atomic.rs:48-128`), so the old direct-write finding is obsolete. However, fixed `.config.toml.tmp` and `.config.toml.restore.tmp` files use create/truncate opens that follow symlinks. The sentinel overwrite in §1 demonstrates the normal-write path. `config_root()` and `data_root()` still fall back to the working directory (`persistence.rs:110-119`).

**Fix:** create a unique sibling temporary file with exclusive creation and restrictive creation-time permissions, retain ownership of that file through sync/rename, and apply the same rule to rollback. Keep the existing lock and durability semantics. Return a path-discovery error instead of silently writing under CWD. Treat lock-file and parent-directory trust explicitly if hostile/shared config directories are supported.

**Done when:** a pre-existing temporary symlink cannot alter its target; failures preserve the previous config; recovery writes satisfy the same checks; a missing configuration root does not write into an arbitrary project.

### AR-04 — Fix the remaining crash/input-lifecycle defects

**P1 · Mixed evidence · CQ-1, CQ-2, CQ-4, CQ-5**

- **Title slicing — Resolved; Source + Observed.** Local title generation now stops at five words or 16 Unicode scalar values, then adds an ellipsis when truncated. It does not slice UTF-8 at an arbitrary byte or collect all input words. Cyrillic/CJK/emoji regression cases pass; a real terminal conversation with a Japanese/emoji title streamed successfully. This limit counts characters, not terminal columns or grapheme clusters.
- **Memory export — Source.** `src/app/export.rs:141-143` still calls `blocking_read()` from async dispatch. Use an awaited short-lived read or a fallible immediate snapshot, releasing the guard before I/O. Acceptance: export a real memory through the dialog under Tokio; no panic, correct content, useful error on failure.
- **Ctrl+E routing — Source.** `src/app/input_events.rs:271-274` still precedes ordinary save-dialog, editor, and viewer handlers at lines 276-325. Move the composer shortcut behind those owners. Do not build a focus-stack framework just to fix ordering. Acceptance: Ctrl+E reaches the editor/viewer as intended, does not replace the draft editor, and cannot open one behind another modal.
- **Terminal teardown — Observed; partly improved during AR-01.** `main` now restores the terminal before propagating an error from `app.run()`, including a cancellation-save failure. Storage initialization can still return after raw mode starts, and the panic hook is not a general terminal-session guard. Use a real RAII guard that also handles partially completed setup; load storage before terminal activation where possible. Acceptance: startup storage errors, draw errors, normal quit, and panic all restore terminal state. The original isolated startup-storage failure remains an open concern.

### AR-05 — Finish the settings cutover; do not rebuild settings

**P1 for unsafe writes; P2 for drift · Source · SET-3 through SET-8, SET-12**

The new implementation already has a settings catalog and real editors (`src/ui/settings_workspace.rs:82-152,756-839`), common keyboard/mouse intents, and persist-before-publish semantics (`src/app/settings.rs:62-111,177-191`). Overrides are consulted by live input dispatch (`src/app/input_events.rs:253-256`; `settings_workspace.rs:1023-1037`). Old claims that only four settings work or keybindings are ignored are no longer true.

Remaining defects:

- `toggle_collapse_thinking` retains the default-config fallback, saves it, and ignores a failed live write (`src/app/runtime.rs:1337-1347`). The dangerous outcome is latent; current normal execution was not shown to contend on that lock.
- Startup forces `ui.kitty_enhanced_text = false` (`src/app.rs:125`), while settings publishing copies the real setting (`src/app/settings.rs:121`). Behavior can differ before and after changing an unrelated setting.
- Sidebar actions change panel state without saving it (`src/app/runtime.rs:234-252`), although the settings page exposes persistent sidebar modes.

**Fix:** route remaining setting mutations through the existing validated persistence/publish path. Reuse a common initial/live UI application function where behavior should match. Persist sidebar toggles if they are meant to change the saved preference; otherwise document temporary visibility separately. A by-value `AppConfig` may simplify ownership later, but it is not a prerequisite for closing these defects.

**Done when:** a busy config never writes defaults; failed saves leave live state and disk unchanged; startup and live settings agree; sidebar restart behavior matches the documented policy. Preserve editable drafts and the new manager boundaries.

### AR-06 — Surface dispatch failures without corrupting connection status

**P1 · Source · CQ-7, CQ-8**

The AR-01 integration now reports errors from keyboard, mouse, and queued dispatch through toasts. This closes the three discarded-result boundaries. Informational status updates still mark the connection failed, while successful file/link actions can mark it connected without a connectivity check. Error redaction and the remaining operation-specific silent failures still need review; this finding is not fully closed.

**Fix:** one dispatch/error boundary shared by keyboard, mouse, and queued actions. Show a redacted error notice and record diagnostic context. Reuse the toast channel for operation feedback; reserve connection state for connection results. Do not duplicate a toast for operations already handling and returning their own user-visible failure.

**Done when:** failed chat persistence/export/delete is visible through all input paths; no success notice appears after failure; informational notices do not change connectivity indicators; diagnostics do not expose credentials or private response bodies.

**Memory write/index follow-up — Source:** `MemoryStore::write` persists the encrypted document before fallible index synchronization. The contextual follow-up adds typed saved/removed-but-not-indexed outcomes, and its skill runner reports the completed mutation with an index warning. Final-answer provider failure also preserves host activity. Keep auditing other callers and operation-specific errors; this does not close the broader AR-06 finding.

### AR-07 — Enforce authority for URLs and local media, not just argument escaping

**P1/P2 · Source · SEC-2 plus a current media-import concern**

1. **External links:** `src/app.rs:555-579` still passes model-provided targets directly to `xdg-open`/`open`. This is not automatically shell injection—there is no shell—but it does delegate arbitrary schemes/options to external handlers.
2. **Automatic local media import:** finalization examines model text (`src/app/chat_files.rs:8-40,108-124`). `local_media_sources` accepts local image-looking paths (`src/app/artifacts.rs:327-370`), and `import_for_chat` reads the source without a trusted-root or provenance check (`src/storage/chat_files/mod.rs:81-91`). [INFERENCE] A model can cause a known private local image to be copied into Saved files merely by naming it. A `.png` symlink can also reference non-image bytes because import does not decode the source. This is unauthorized local reading/persistence, not demonstrated remote exfiltration.

**Fix:** parse external URLs and permit HTTP(S) by default. Treat `skill:` as a separate internal action; add `mailto:` only if intentionally supported. Reject option-like and malformed targets before spawning, and expose the destination. For media, allow automatic access only to files registered as outputs of trusted host operations; require explicit user selection/approval for other local paths. Apply the same authority rule to previews, not only saving. Reuse confined filesystem operations rather than relying on extensions.

**Done when:** blocked schemes/options spawn nothing; ordinary HTTPS links work; an assistant-emitted outside-root image or symlink cannot trigger automatic reading/copying; approved generated images still import once. Keep the recent explicit command-review requirement for terminal execution; do not replace it with a “safe shell text” heuristic.

### AR-08 — Require explicit host authorization for memory writes

**P1 · Implemented follow-up · Source + Observed · SEC-3**

Before the fix, a real terminal probe saved an assistant's inline-code `<tcui:remember>` example despite `memory.auto_capture = false`. The displayed answer also lost the directive text.

The owner selected **explicit saves only** pending a possible replacement of the memory system. Automatic capture and its setting are removed, not retained as an inactive switch. The response filter, implicit capture API, and directive prompts are gone.

**Corrected contract:** the literal-prefix implementation did not match the owner's intent and has been removed. Inline `@memory`, `@remember`, `@memorize`, and `/skill <name>` select a contextual skill. The model receives the full current prompt and conversation, then selects recall, save, or a clarification question from the user's meaning. “I do not @remember” is a recall request, not a save instruction.

The host captures skill selection from the raw typed instruction before adding documents, web results, or recalled context. A clarification can continue through host-authored activity on the previous answer in the same request destination; quoted text and another chat's copied history do not grant that authority. `@recall` and `/skill recall` enforce read-only actions in the host. Other memory skills rely on the model to interpret save versus recall intent; do not describe that interpretation as a deterministic host guarantee.

The host validates structured actions, checks Memory availability, and executes real store operations. It rejects malformed plans, read-only mutations, oversized input, and recognized credential patterns. It reports completed save/removal separately from indexing failure. Saved activity and pending clarification survive a failure of the final answer provider. Literal answer tags still cannot execute actions.

The old 500-character literal-fact limit is gone with that API. Contextual Markdown and plan/input limits are 1 MiB. Bounded tool results report truncation. Memory file operands remain separate from natural-language requests to copy a temporary pane.

### AR-09 — Complete the Vault filesystem cutover

**P2 · Source · SEC-1, SEC-4**

The old `backup_dir.join(model_argument)` restore implementation is gone. Native operations use `StandaloneBackupId` with a fixed `write-` plus 32-hex format (`src/vault/backup.rs:20-46`), matching profile/path checks (`mutation_plan.rs:71-87`), and confined `VaultFs` operations with no-follow traversal (`src/vault/fs.rs:337-356,504-507`). Do not reimplement the old suggested validator or downgrade this to canonicalize-then-open.

Legacy access remains: `src/obsidian/vault.rs:34-44` follows normal paths; artifact writing calls it at `src/app/artifacts.rs:161-170`. Artifact insertion canonicalizes a path and then reopens the original path (`src/app/pending_documents/artifact.rs:73-82`), leaving a check/use gap. Reachability of every old temporary-artifact flow was not established, so this is not a claim that the original model-driven overwrite scenario remains intact.

**Fix:** migrate remaining real reads/writes to the existing confined service; remove unreachable adapters rather than adding another safety layer. Keep preview/revalidation/confirmation for mutations. The backup store's own filesystem trust is a separate boundary; a validated ID alone does not prove the store is immune to tampering.

**Done when:** every live Vault read/write uses the same confinement policy; final and ancestor symlinks cannot escape it; restore validates the selected profile/path; no old raw `Vault::write_file` caller remains.

### AR-10 — Bound network input and make insecure transport intentional

**P2 · Source · SEC-7, SEC-10**

- Custom API endpoints still accept non-loopback HTTP (`src/llm/auth/policy.rs:27-40`). Automatic environment/`.env`/other-app credential discovery is gone (`src/llm/auth/resolver.rs:38-43,79-101`). The remaining problem is transport policy for explicitly saved credentials, not ambient credential theft.
- Generic SSE accumulates bytes without a size bound and waits through any UTF-8 error (`src/llm/chat.rs:530-573`). A timeout is not a memory bound. Codex already has total/event/error-body limits (`src/llm/codex_responses/transport.rs:11-24`).
- ntfy still defaults a bare host to HTTP (`src/notifications.rs:169-172`), but delivery now has a five-second timeout (`367-370`).

**Fix:** require HTTPS for remote credential-bearing endpoints by default. Preserve loopback use and, if needed, a clearly disclosed explicit insecure-LAN exception; do not break local inference indiscriminately. Apply finite streaming/event/error-body limits across transports, checking before growth; distinguish incomplete UTF-8 from invalid encoding. Default remote notifications to HTTPS or require explicit HTTP configuration.

**Done when:** remote bearer credentials are not sent over HTTP without deliberate permission; loopback remains usable; huge unterminated events and malformed UTF-8 fail with bounded memory; normal split UTF-8/SSE frames still stream correctly; notifications remain deadline-bounded.

### AR-11 — Separate update-path hardening from release-signing policy

**P2 · Source · SEC-5**

`src/updater.rs:64-74` creates a reusable temporary path with `create_dir_all`; downloads use `fs::write` (`126-135`). `scripts/install.sh:79-86` still continues when checksums cannot be downloaded. The API's `browser_download_url` is accepted directly (`updater.rs:114-122`).

**Fix now:** exclusively create a private staging directory, use no-follow/exclusive file creation, validate expected HTTPS release URLs, and fail closed on missing checksum material. Account for legitimate GitHub download redirects to its release-asset infrastructure; a naive “every hop must be github.com” rule will break downloads.

**Separate decision:** signed release metadata or attestations can improve authenticity, but only with an independently trusted verification key/identity and a release/rotation process. A signature fetched beside the binary with no pinned trust root does not solve the problem. TLS plus GitHub is an existing trust model; unsigned checksums are not proof of a currently exploitable compromise.

**Done when:** staged symlinks/pre-existing directories cannot redirect writes; missing/mismatched checksums abort before installation; approved release redirects work; a signing rollout verifies the expected publisher rather than merely any valid signature.

### AR-12 — Update verified affected dependencies, with reachability-aware reporting

**P1 for active TLS fixes · Source + upstream advisories · DEP-1, DEP-2**

The following versions remain in the current lockfile. These primary advisories were checked during this review:

| Crate | Locked | Verified patched range | Evidence / qualification |
|---|---:|---:|---|
| rustls | 0.23.38 | >= 0.23.46 | Active TLS dependency. [RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285.html) is fixed in 0.23.45; [RUSTSEC-2026-0349](https://rustsec.org/advisories/RUSTSEC-2026-0349.html) requires 0.23.46. |
| rustls-webpki | 0.103.13 | >= 0.103.16 within 0.103.x | [RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382.html): distrusted intermediate handling. |
| webbrowser | 1.2.1 | >= 1.2.2 | [RUSTSEC-2026-0257](https://rustsec.org/advisories/RUSTSEC-2026-0257.html). `hardened` is already enabled. The model-link opener in AR-07 bypasses this crate, so updating it does not fix SEC-2. |
| crossbeam-epoch | 0.9.18 | >= 0.9.20 | [RUSTSEC-2026-0204](https://rustsec.org/advisories/RUSTSEC-2026-0204.html). Active dependency; vulnerable formatting-path use was not established. |
| quinn-proto | 0.11.15 | >= 0.11.18 | [RUSTSEC-2026-0338](https://rustsec.org/advisories/RUSTSEC-2026-0338.html). Locked, but not selected in this host's all-features dependency tree. |

**Fix:** use targeted compatible lockfile updates, then run a current advisory scan and normal verification. Prefer that over an unrestricted `cargo update` mixed into application fixes. Record remaining advisories with actual dependency/feature reachability; do not repeat “12 vulnerable versions” as a fresh result. The old `anyhow`, `lru`, `paste`, and `number_prefix` warning classifications were not independently revalidated here.

Add advisory checking to CI. A license-policy `cargo deny` rollout is a separate maintenance choice, not required to fix these vulnerabilities. Move Tokio's test-only feature activation into dev-dependencies if no runtime consumer requires it (`Cargo.toml:21`).

**Done when:** a current scanner and dependency tree establish the remaining risk; selected TLS versions are patched; normal HTTPS and provider flows still work; exceptions identify affected targets/features and an owner.

### AR-13 — Preserve literal text and separate it from host actions

**P1 · Implemented follow-up · Source + Observed**

The renderer previously discarded HTML events and treated namespaced tags as URL autolinks. Separate answer filters removed title/memory tags, executed quoted examples, and discarded text after an unfinished opening tag. The filtered answer was then saved.

HTML events now render as literal text. XML-name-like namespace autolinks remain literal, including combining characters and middle dots; ordinary web links, explicit Markdown links, and code formatting remain supported. Existing Markdown paragraph whitespace rules are unchanged.

All provider transports now stream answer text without title filtering. `TitleTagFilter`, title stream/actions, request title mutation, and `RememberFilter` are removed. Local Unicode-safe titles preserve fork names. Unfinished tags are ordinary text, not growing command buffers. An empty SDK text delta no longer suppresses a later complete result answer.

Real terminal proof covered user tags, quoted assistant directives, an HTML block, unfinished directives with a trailing explanation, Unicode titles, and exact answer preservation after restart. These changes cannot recover answer bytes discarded by older versions.

### AR-14 — PR/CI enforcement and coverage

**Owned by another session**

The owner is implementing CI/CD separately. This tag-safety change does not edit workflows, branch rules, or release policy. Reconcile the earlier CI/release findings with that session's final results rather than applying stale recommendations in parallel. Hosted protection and release enforcement are not verified by this session.

### AR-15 — Review or replace the memory system

**Contextual workflow implemented · Current backend retained · Remaining management and encryption work is explicit**

The owner approved contextual memory implementation now and an encryption migration plan for a later session. No replacement memory backend was selected. Keep SQLite/sqlite-vec and existing records. Do not restore hidden save tags or automatic capture.

#### Current behavior

- Inline `@remember`, `@memory`, `@memorize`, and `/skill <name> <request>` load skills and use the full prompt plus conversation. The model selects save versus recall from meaning. It can save a useful configuration note instead of a literal suffix. `@recall` is a host-enforced read-only selection, including mixed-skill prompts.
- Missing facts or an unclear machine produce a clarification question. A plain reply can resume the host-owned task in the same conversation, including after restart. Authorization is captured before synthetic tool messages are added. A new topic ends the task without mutation. Model interpretation remains model-dependent; structured validation is not proof that every model will infer intent correctly.
- Contextual records have user/machine scope, machine ID/name where applicable, topic tags, and source-conversation title/ID metadata. The local identity uses the TCUI execution host's machine ID and hostname. Named targets reuse saved identities or use a label-derived ID; this is not verified remote inventory or a machine-renaming system.
- Source titles are snapshots. Persistent conversation IDs are recorded when available. A first-turn save can precede chat ID allocation, so its source has a title but no ID. Temporary sources retain a title without a reusable negative chat ID. Record creation time is not proof of the machine's setup date.
- Existing encrypted records without metadata remain readable and are not silently assigned a scope. Scope and machine identity participate in duplicate detection. A duplicate keeps its existing tags/source; the host reports that proposed metadata was not applied.
- Explicit search filters scope/machine candidates before vector ranking. Selected hits include metadata and decrypt only selected files. However, synchronization still scans and decrypts active records before each search. This is not yet a direct-query-only hot path.
- Explicit memory tasks skip the separate automatic memory injection. Manual retrieval uses at most four planning steps, eight hits per search, and 16,000 characters per tool result. Ordinary automatic recall retains its two-memory/320-character/0.55 defaults. Memory search is semantic; the separate AR-01 conversation-history search is lexical.
- The right sidebar preview shows stored metadata. Insertion still copies the selected note into the next request and saved history. Requested removal moves a record to Trash and refreshes the sidebar. Sidebar Delete still directly removes the file and ignores filesystem errors; that inconsistency remains.
- Save/removal outcomes distinguish durable mutation from an indexing failure. The final provider answer can fail without erasing the host's completed activity or pending clarification state.
- Records use the existing local-key encrypted files. The search index is not password-encrypted, and credential-vault unlock does not protect this data. See AR-17; do not claim app-wide password protection exists.

#### AR-15A — Memory management and source context

**Implemented:** source title/optional ID, scoped metadata, metadata preview, persistent clarification state, and command-driven removal refresh.

**Remaining:** user-controlled sorting and scope/machine/subject/source/date filters; consistent command/UI deletion; clear Trash versus permanent deletion; visible errors; and full view/index refresh after every mutation. Keep copied chat content separate from memory deletion. Add stable message/request references when useful. Do not retain entire temporary chats merely to explain one saved fact.

#### AR-15B — One skill, user and machine scope

**Implemented:** the same contextual skills handle both scopes. The host supplies local identity and source details. Machine records include a readable name and machine tag. Scope is checked before storage.

**Remaining:** richer machine identity/alias management if needed, including rename/conflict handling for named remote targets. Keep multi-device synchronization a separate decision. A user-scoped fact in this local store is not automatically available on another computer.

#### AR-15C — Selective recall and efficient indexing

**Implemented:** inline and slash skill selection, contextual recall, a strict read-only alias, metadata-bearing results, pre-ranking scope filters, bounded manual retrieval, and no second automatic injection for explicit memory tasks.

**Remaining:** an independent automatic-recall switch; machine-aware automatic recall policy; deduplication against manually inserted notes; and mutation-driven indexing with bounded startup/reconciliation instead of a decrypt-all scan per search. The older terminal probe confirmed duplicate insertion plus automatic recall for ordinary prompts; that case is still open.

Keep durable facts, complete chat history, and selected excerpts separate. Do not fill a request with complete unrelated conversations. Compare replacement backends only if the owner later chooses to do so; require explicit data migration/export rather than silent deletion.

#### Contextual workflow runtime evidence

An isolated terminal and local provider fixture exercised the owner's local-CI example. The planner request contained the earlier setup command and cache path; the resulting record contained those facts, not the text after `@remember`. A separate real memory MCP process read the persisted machine scope, identity, tags, source ID, and source title.

The “I don't @remember” example performed a search without modifying record files. A missing backup schedule caused a question and no save. After application restart, a plain answer completed the save. A deliberately invalid mutation from `@recall` and a malformed plan created no record. A completed save remained visible when the final provider response failed. The memory viewer displayed the stored metadata.

These checks use controlled provider responses and real TCUI planning, persistence, query, restart, and UI paths. They do not measure live-model semantic accuracy.

**Done when:** the remaining management/retrieval requirements have demonstrated behavior, the encryption boundary is handled under AR-17, and any later retain/replace/remove decision has explicit data disposition.

### AR-16 — Reconcile and clear strict Clippy failures

**Locally cleared in the contextual-memory worktree · Integration/CI reconciliation remains**

The earlier `cargo clippy --locked --all-targets --all-features -- -D warnings` run failed on 16 diagnostics with Fedora Rust 1.99.0, while the repository then pinned 1.96.0. Both strict Clippy configurations now pass with the current **1.99.1** pin in `fix/contextual-memory`; see §1. The CI/CD session supplied intervening fixes and toolchain alignment. Do not claim those changes originated in this memory task.

Historical inventory; line numbers refer to the earlier failing run:

| Diagnostic | Count | Location |
|---|---:|---|
| `needless_return` | 1 | `src/app/vault_mutation_execute.rs:177` |
| `unnecessary_get_then_check` | 2 | `src/app/request_only_document_tests.rs:446`; `src/app/tests/insert_to_chat/lifecycle.rs:57` |
| `field_reassign_with_default` | 1 | `src/app/tests.rs:1675` |
| `await_holding_lock` | 5 | `src/app/tests.rs:1748`; `src/llm/model_fetcher_tests.rs:136,196,238`; `src/memory/mcp_server/tests.rs:12` |
| `ptr_arg` | 2 | `src/storage/db.rs:882,889` |
| `module_inception` | 1 | `src/tui/vault_profile_ui.rs:21` |
| `collapsible_str_replace` | 2 | `src/ui/components/markdown_rich_tests.rs:205,207` |
| `items_after_test_module` | 1 | `src/ui/components/terminal_capabilities.rs:302,384` |
| `cmp_owned` | 1 | `src/main.rs:558` |

**Remaining action:** reconcile the isolated worktree with the CI/CD session and verify the actual integrated candidate. Preserve shared environment-lock isolation in async tests. Do not drop guards across `.await` merely to silence a lint, add blanket suppression, or weaken `-D warnings`.

**Done when:** strict Clippy succeeds on the integrated candidate with the pinned toolchain and required feature configurations. Record the commit and CI outcome then. The implementation checkpoint predates publication; local success alone does not establish a remote CI result.

### AR-17 — One startup unlock for optional app-wide encryption

**Planned migration · Explicitly deferred by the owner for this run**

The required product behavior is a choice for the Linux user, not a TCUI account login. If the user enables password protection, the password unlock must protect all TCUI-owned usage data, including memories and their database/index. If the user declines protection, the UI must state that clearly.

**Current gap:** `KeyStore` uses an Argon2id-derived password key for credentials. `SharedKey::load_or_create_default` instead reads a local `chat.key` for chats and memories. The memory index uses ordinary SQLite. Startup creates storage and loads chat data before `prompt_credential_vault_at_launch`. The existing credential prompt is not an app-data unlock boundary.

**Migration plan:**

1. Inventory app-owned content and metadata: conversations, titles, memories, vector indexes, credentials, saved files, link manifests, state databases, configuration containing private values, Trash, backups, temporary files, and journals/WAL files. Define the boundary for external Markdown Vault files and explicit exports before implementation. Do not silently encrypt or rewrite unrelated user files.
2. Use one application protection choice and unlock lifecycle. Load only minimal non-secret bootstrap/KDF metadata before unlock. Do not open chat/sidebar/memory content or build an index while locked. Cancellation or a wrong password must not fall back to an unprotected store.
3. Reuse audited cryptographic primitives and the existing password/KDF patterns. Protect the data key with the password-derived key; do not leave a plaintext key file that bypasses the password. Protect the search database and its sidecars, not only source Markdown. Do not choose a new memory backend merely to add encryption.
4. Apply the same gate to CLI exports/imports, the memory MCP server, and future scheduled tasks. A headless caller must use an explicitly authorized unlocked session or fail closed. Never place passwords in process arguments, model prompts, logs, or ordinary configuration.
5. Design migration and recovery before converting existing data. Require verified backups, authenticated format/version checks, staged conversion, and interruption recovery. Enabling, disabling, or changing protection must not strand records or silently discard the old key.
6. Demonstrate protected and unprotected startup, wrong-password/cancel behavior, restart, key change, damaged data, interrupted migration, memory search, Trash, exports, MCP access, and temporary-chat saves. Check the filesystem for plaintext content, metadata, index sidecars, and stray temporary copies.

No migration or password/key change was performed in this contextual-memory run. Preserve the existing key and records until this plan is implemented and verified.



## 3. Secondary work queue

These are actionable but should not block the correctness/security cutover above.

### Storage and background work — P2, CQ-9/10/11/16

- Consolidate duplicated database initialization and ignored `ALTER TABLE` errors (`src/storage/db.rs:39-89`). Use ordered transactional migrations with a version marker. Opening an old DB must either finish the migration or return an actionable error without a half-migrated schema.
- Stop background model refresh from reopening full application storage and nesting `block_on` inside `spawn_blocking` (`src/app.rs:174-190`; `src/app/providers.rs:205-210`). Fetch remotely in async code; pass results back to the existing owner and isolate synchronous DB work. Multiple SQLite connections are not inherently a bug, and an async network loop hosted on a blocking thread is not inherently a deadlock; repeated initialization and ownership complexity are the concrete problems.
- Move startup refresh out of `TuiApp::new`. Its `stdin().is_terminal()` guard still does not establish a Tokio runtime (`src/app.rs:174-176`; CQ-14). Constructing the app in a TTY-backed synchronous harness must not panic or start unexpected network work.
- Export one conversation by ID instead of decrypting every chat (`src/app/export.rs:129-138`). Profile sidebar listing before adding an index. If needed, keep a rebuildable local index rather than another authoritative store, and preserve encryption/privacy expectations for titles.
- Model refresh now returns an error for an empty result and saves only a successful catalog (`src/llm/model_fetcher.rs:304-335`); the original “silent empty dropdown” claim is overstated. Preserve HTTP/auth/parse error detail inside fetch operations and distinguish background refresh failures (`338-370`). Acceptance: a 401 preserves cached models and produces an actionable message without exposing credentials.

### Rendering — P2 after measurement, CQ-6

Full message building remains on the redraw path, twice when adding a scrollbar (`src/ui/chat_tab.rs:451-461`), and the loop still ticks every 33 ms (`src/app/runtime.rs:39-45`). However, diagram layout is already bounded and cached (`src/ui/components/markdown_rich.rs:247-281`), and media import has moved to finalization. “No cache anywhere” and “import media every token” are not current descriptions.

Measure long conversations containing diagrams and shell blocks on the current renderer. Cache reusable per-message parse/layout work with explicit invalidation for content revision, viewport width, theme, fold state, render mode, and terminal capabilities. Avoid hashing the full history every frame. Preserve hit areas and source-based copying when reusing layout. Reducing unnecessary redraws must retain PTY output, credential-login polling, Vault reconciliation, and animated feedback; do not simply remove the tick.

Acceptance: unchanged long chats do not repeatedly parse/layout their contents; streaming, resizing, theme/fold changes, command controls, and active terminals remain responsive and correct. Benchmark before assigning a performance severity.

### Smaller hardening items — P2/P3, SEC-8/9/10

- **CWD trust:** `load_system_prompt` now embeds a fallback but still prefers `assets/TCUI.md` from CWD (`src/app.rs:255-257`). Skill discovery also scans CWD and other apps' directories (`src/skills/catalog.rs:18-48`). Prefer embedded defaults plus explicit trusted override locations; use an explicit project trust decision for local skills. Built-ins are loaded first, so do not claim external files overwrite every built-in. Do not hash-pin an unrelated installer lockfile as an improvised trust policy. Acceptance: launching inside an untrusted checkout does not silently replace system instructions.
- **Auth test marker:** `src/auth_command/runner.rs:69-70` still creates/truncates an environment-selected path. Remove the production write or gate the instrumentation appropriately, preserving integration-test synchronization. `cfg(test)` alone does not apply to the normal binary launched by integration tests; debug gating also needs care if release integration tests are supported. This is a low-severity environment-controlled hazard, not an independent privilege escalation.
- **Draft files:** `src/app/input.rs:20-21,249-254` still writes into the temporary directory with ordinary creation permissions. Exclusively create a private draft at 0600, retain lifecycle ownership, and ensure cleanup cannot delete a replacement path. Acceptance: private text is not exposed under a permissive umask and an existing symlink is not followed.
- **Reminder store:** `src/reminders/store.rs:48-50` still directly rewrites metadata. Use atomic replacement with a clear concurrent-update policy. Acceptance: interrupted writes do not lose the existing schedule metadata.
- **Shared-key writes:** `src/storage/crypto.rs:207-217` uses exclusive creation but changes permissions only after writing. Set restrictive mode at creation. The usual data directory is already 0700, so do not inflate this into a demonstrated cross-user leak.
- **Diagnostics:** `src/diagnostics.rs:33-51` needs private creation permissions/path handling, not whole-file atomic replacement on every log append. Keep redaction and append semantics.
- **Plaintext compatibility:** `decrypt_shared_text_with_key` still accepts un-enveloped text (`src/storage/crypto.rs:130-136`). Inventory actual stored formats and implement an explicit migration before rejecting old data. Do not break existing reminders merely by removing the fallback.
- **Gemini file permissions:** the old automatic token-file reader was removed by the explicit-credential cutover. Do not add permission checks to a deleted import path; review any actual opt-in importer separately.

### Dead code, command metadata, and docs — P3

- Keep the new settings catalog. Consolidate duplicated command metadata incrementally across navigator, keybindings, slash completion/parser, and help. Keep command execution context-aware: “open,” “close,” and “toggle” are not interchangeable. The old pinned palette can be removed after its live references are checked; do not delete working keyboard override support.
- Do not mechanically apply DEAD-1/2. MCP profile exports now have consumers (`src/app/credential_manager.rs:574,604,1178`; `src/config/app_config.rs:580`). `notify` is now a dependency used by `src/vault/watcher.rs`. The old `src/obsidian/watcher.rs` is a different implementation. Several old components remain declared in `src/ui/components/mod.rs`; compilation and actual usage must be distinguished.
- Remove the unused `_llm: Arc<LlmClient>` constructor parameter and its allocation if its lack of use remains confirmed at implementation time (`src/app.rs:86-90`; `src/main.rs:195-199`). Retire dead actions before making dispatch exhaustive; the current `_ => Ok(())` hides omissions (`src/app/runtime.rs:581`).
- Narrow blanket dead-code/unused-import suppression module by module. Use compiler/reference evidence across both feature configurations; do not delete serialized fields or database tables merely because a text search finds no `SELECT`.
- The DB's providers/models cache is not automatically a harmful second source of truth. Remove misleading unused mutators, then document ownership. Drop genuinely obsolete tables only through an explicit migration; that deletion is not a prerequisite for fixing current behavior.
- Update `ARCHITECTURE.md` selectively. It still claims “47 source files,” documents `stream.rs`, and describes chats/settings as SQLite-backed (`10,87-113,347-355`), while other sections correctly describe newer auth/search behavior. Keep useful design history in `docs/ui-refactor-plan.md`, but label historical claims and replace the unsupported general renderer-cache assertion at line 82.
- Do not standardize every test file layout just for consistency. Environment isolation and guards that restore state on panic matter more than inline versus sibling test modules. The old exact warning count and dead-code line count need a new compiler run before use as acceptance targets.

### CI coverage — P2, CI-1/2, TEST-1/2

`.github/workflows/ci.yml:16-19` still omits `--all-targets` from clippy and then calls `evals/smoke.sh`, which repeats check/test work. Make one authoritative check/test pass, add all-target clippy and a no-default-features lane, and replace the headless refusal check with an isolated PTY startup/quit/error-cleanup smoke. Keep a small headless contract check if its refusal behavior is intentional, but do not call it UI coverage.

Acceptance: CI exercises terminal teardown on failure, catches feature-specific config preservation and test-target lints, and does not duplicate the same full suite unnecessarily.

## 4. Disposition of the earlier report

“Partial” means the topic remains useful but its scenario, scope, or proposed fix changed. “Deferred” is not a finding of safety.

| Original IDs | Current disposition | Action |
|---|---|---|
| SEC-1 | Superseded by typed restore plans and native Vault mutation flow | AR-09; retain restore/confinement checks, do not patch removed code. |
| SEC-2 | Still valid | AR-07. |
| SEC-3 | Addressed by explicit-save-only cutover | AR-08; complete system review remains AR-15. |
| SEC-4 | Partial: native filesystem is confined; legacy adapters remain | AR-09. |
| SEC-5 | Still relevant, but signing and local filesystem safety are distinct | AR-11. |
| SEC-6 | Resolved for the named paths | Local search no longer invokes ddgr; notify-send now uses `--` at `src/notifications.rs:281`. |
| SEC-7 | Partial | Ambient discovery removed; remote HTTP credentials remain AR-10. |
| SEC-8 | Still a trust-policy issue | CWD/trusted-source work in §3. |
| SEC-9 | Still present | Test-marker hardening in §3. |
| SEC-10 | Mixed | SSE/ntfy in AR-10; file writes/migration in §3; Gemini reader removed; ntfy timeout fixed. |
| SET-1 | Superseded | Config bootstrap no longer rewrites keys; credential resolution no longer reloads config; `KeyStore::get` now reads SQLite and is not dead. |
| SET-2 | Still valid with corrected comment behavior | AR-02. |
| SET-3 | Partial | Settings writes are safer; collapse toggle and memory-export blocker remain AR-04/05. |
| SET-4 | Superseded | Real values come from the loaded settings workspace; old static bool-toggle diagnosis no longer applies. |
| SET-5 | Mostly superseded | Keyboard/mouse workspace intents share one path; navigator Enter matching remains duplicated but no longer has the old theme/bool divergence. |
| SET-6 | Partial | Command metadata/palette cleanup remains; settings are now a dedicated workspace, not merely the mislabeled old panel. |
| SET-7 | Superseded | Catalog, typed validation, real editors, and manager routing now exist. |
| SET-8 | Superseded | Live dispatch consults keyboard overrides. Keep and verify current behavior. |
| SET-9 | Partial | Cache ownership/unused APIs remain maintenance work; table removal needs migration evidence. |
| SET-10 | Partial | Several legacy fields are explicitly listed as inactive in `settings_workspace.rs:54-80`; keybinding overrides and `provider_config` now have real consumers. Kitty startup drift remains. |
| SET-11 | Partial | Atomic save implemented; temp-file safety and CWD fallback remain AR-03. |
| SET-12 | Partial | Publishing is centralized for the settings page; startup/sidebar drift remains AR-05. |
| SET-13 | Mostly superseded | Old bool/reset scaffolding diagnosis is stale. Navigator `all_settings()` still allocates; optimize only after the larger rendering cost is measured. |
| CQ-1, CQ-2 | Partial: Unicode title fixed; memory export remains | AR-04. |
| CQ-3 | Addressed in the AR-01 follow-up | Request-owned snapshots and routes; switching preserves background streams. |
| CQ-4 | Still valid for ordinary editor/viewer/save overlays | AR-04. |
| CQ-5 | Partial fix only | Config moved earlier and `app.run()` errors now restore the terminal; startup storage failure still needs the AR-04 guard. |
| CQ-6 | Partial | Full-history layout remains; diagram caching and finalize-only media handling now exist. |
| CQ-7, CQ-8 | Partly addressed | Dispatch errors now reach toasts; remaining silent operations and connection-status semantics remain AR-06. |
| CQ-9, CQ-10, CQ-11 | Still useful, with narrower implementation advice | Storage queue in §3. |
| CQ-12 | Addressed in the AR-01 follow-up | Abort handles, double Escape, explicit cancellation, stale-event rejection, and interrupted-response persistence. |
| CQ-13 | Partial | Developer absolute path removed, embedded fallback added; CWD override remains. |
| CQ-14 | Still source-supported; not reproduced in a TTY test suite | Move background startup out of constructor. |
| CQ-15 | Partial | Conversation listing returns skipped-file counts; remaining stderr paths need caller-specific review, not a claim about every sidebar refresh. |
| CQ-16 | Partial | Empty refresh now returns an error and preserves cache; fetch-layer error classification still needs work. |
| CQ-17 | Mixed maintenance candidates, not a single defect | Consolidate commands/ownership where it prevents drift; avoid a broad deduplication rewrite. |
| CQ-18 | Partial | New submission/terminal/settings modules already split responsibilities; dispatch catch-all and input precedence still deserve focused changes. |
| CQ-19 | Mixed, lower priority | Default-key helpers, theme-lock assumptions, per-request clients, and token limits need individual contracts. `SetProviderModels` now consumes its payload; old PTY implementation references are stale. No blanket refactor proposed. |
| DEAD-1, DEAD-2 | Stale as a deletion manifest | Recheck item consumers and serialized compatibility before removing anything. |
| DEAD-3 | Directionally sound | Narrow suppressions incrementally; verify both feature builds. |
| DEP-1 | Still necessary, with targeted updates | AR-12; do not claim a completed current audit. |
| DEP-2 | Advisory CI recommended | Keep license policy separate. |
| DEP-3 | Still present in manifest | Move test-util activation if runtime consumers do not require it. |
| DOC-1 | Partial documentation refresh, still materially stale | Correct surviving storage/module claims, not wholesale deletion. |
| DOC-2 | General-cache claim still misleading | Distinguish existing diagram cache from absent full-message caching. |
| CI-1, CI-2 | Still relevant | Remove duplicate runs; broaden clippy and real smoke coverage. |
| TEST-1 | Partly agree | Prioritize deterministic environment isolation over one mandatory file layout. |
| TEST-2 | Historical result only | Not evidence for the current tree. |

## 5. Recommended implementation order

**Completed:** AR-01 request ownership, cancellation, recovery, forks, and bounded cross-conversation recall; AR-08 explicit memory authorization; AR-13 literal text and action separation; Unicode title safety within AR-04. Preserve these contracts during later asynchronous or rendering changes.

1. **Configuration patch:** AR-02, AR-03, and remaining mutations in AR-05. Preserve the new settings workspace and existing migration semantics.
2. **Crash and visible-error patch:** remaining AR-04 and AR-06 work. Include startup PTY teardown, real memory export, shortcut ownership, and separate operation notices from connectivity.
3. **Model/host trust patch:** remaining AR-07, AR-09, and AR-10 work. Preserve explicit terminal-command approval, explicit memory authorization, and native Vault confinement. Treat local-file reads as authorization decisions, not prompt-engineering fixes.
4. **Dependency/update patch:** AR-12 and immediate AR-11 hardening. Keep dependency churn separate from application fixes; design signing independently if desired.
5. **Memory and storage:** finish AR-15 management/retrieval work while retaining the current backend. AR-17 is the separately approved plan for app-wide password protection; start with the data inventory and migration boundary, not a new login system. Backend replacement still requires the owner's choice.
6. **Performance and cleanup:** §3, after the behavioral contracts above are protected. Measure first; delete only code proven obsolete. CI/CD remains separately owned under AR-14.

The remaining batches overlap runtime dispatch and configuration publication. Use one integration owner or sequence shared-file changes. The original report's broad “independent” batches are no longer a reliable conflict map.

## 6. Next-session handoff

Read this review before restarting work. The publication worktree is `/home/jp/Projects/.worktrees/tcui/contextual-memory` on `fix/contextual-memory`. It includes the completed accumulated baseline, not the paused session's future alignment changes. Preserve the original checkout and follow the publication boundary in §1. Preserve completed request-routing, temporary-chat, literal-text, and contextual explicit-save contracts.

- **AR-01 — Mostly complete:** request preparation is not independently cancellable. Live `/steer` needs a provider transport that supports it; current transports report the limit. Future scheduled tasks must use the existing origin/destination/request ownership contract.
- **AR-02 — Open:** prevent config migration/serialization from dropping unsupported settings.
- **AR-03 — Open:** harden temporary-file creation and atomic writes against symlinks.
- **AR-04 — Partly complete:** Unicode title handling is fixed. Finish memory export, Ctrl+E ownership, and terminal teardown on startup failure.
- **AR-05 — Open:** complete settings mutation, startup/live consistency, and sidebar persistence.
- **AR-06 — Partly complete:** dispatch errors reach toasts; contextual saves/removals report partial index failure and retain host activity after provider failure. Connection-status misuse and other silent failures remain.
- **AR-07 — Open:** enforce authority for URLs and local-media reads.
- **AR-08 — Implemented:** preserve explicit host authorization for memory mutations.
- **AR-09 — Open:** finish the legacy Vault filesystem cutover.
- **AR-10 — Open:** bound network inputs and make insecure transport explicit.
- **AR-11 — Open:** harden updater staging and checksum handling; coordinate release policy with CI/CD.
- **AR-12 — Open:** update verified affected dependencies and check actual reachability.
- **AR-13 — Implemented:** preserve literal tags and keep assistant text separate from host actions.
- **AR-14 — Other session:** PR and CI/CD enforcement. Do not duplicate that work.
- **AR-15 — Partly implemented:** contextual skills, clarification, user/machine metadata, scoped search, and metadata preview work. Management consistency, automatic-recall controls, deduplication, and index efficiency remain. No replacement backend selected.
- **AR-16 — Locally cleared:** strict Clippy passes for all features and no default features with Rust 1.99.1. Recheck the integrated candidate with the CI/CD session.
- **AR-17 — Planned:** implement the optional app-wide password choice, startup unlock, protected memory/index storage, and safe data migration. Explicitly deferred from this run.

Start with AR-02/AR-03 unless integration checks block work or the owner selects AR-17 next. Follow §5 for later batches. Retain the current memory backend unless the owner chooses a replacement.

### Copyable next-session prompt

```text
Continue the TCUI actionable review in /home/jp/Projects/.worktrees/tcui/contextual-memory
on branch fix/contextual-memory.

Read repository instructions and docs/2026-10-22-actionable-review.md first.
Use STE-100 simplified technical English for explanations, and use the
rust-best-practices skill. Run project commands inside the tcui toolbox
with mise and the pinned Rust 1.99.1 toolchain.

This publication branch contains the completed accumulated baseline and
contextual-memory work. The paused workflow-alignment session had made no
source edits. The original /home/jp/Projects/tcui checkout is preserved.
After this PR merges, create new feature worktrees from refreshed
origin/development under ~/Projects/.worktrees/tcui/<feature>.
Do not copy the original checkout's old pending snapshot into the new base.

Preserve the memory contract: @remember, @memory, and @memorize interpret
the entire prompt and current conversation. They select saving or recall
from meaning and ask when required facts are missing. Plain replies can
resume clarification after restart. @recall is host-enforced read-only.
/skill name request works with one Enter. Never restore literal-suffix
saving, automatic capture, or hidden response-tag actions.

Keep user/machine scope, machine names and tags, source title/optional ID,
pre-ranking scope filters, truthful partial outcomes, and persisted host
activity. Keep the current backend unless I choose a replacement.

The owner selected contextual memory now and encryption planning only.
AR-17 describes a future single optional startup password unlock for all
TCUI-owned private data, including memories and indexes. Current local-key
encryption is not that protection. Do not claim it is implemented.

Local checks passed: 1,214 unit + 13 integration tests with all features;
1,170 unit + 13 integration tests with no default features; strict Clippy
in both configurations; all-feature build; formatting check. A real TUI
and controlled local provider exercised persistence, recall, clarification,
restart, read-only rejection, provider failure, and metadata display.
This was not a live-model intent evaluation or remote CI run.

First reconcile the review with current code and restate the remaining
concerns. Recommend one item to handle next, normally AR-02 before AR-03.
Work on one agreed item at a time. Preserve completed streaming, forks,
temporary chats, literal text, and contextual memory. After changes, run
the affected real scenario and checks, update the review, then report the
results and remaining concerns.
```
