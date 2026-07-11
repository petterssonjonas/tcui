# ui-refactor_qna — questions to inform the no-ratatui-kit plan

> Status: questions for the user. Answer inline; the answers feed
> `docs/ui-refactor_no_ratatui-kit-plan.md`, which is drafted after.
>
> Context: the parent plan `docs/ui-refactor-plan.md` (Revision 1) carries a
> "ratatui-kit island" path that is gated on a Phase 0 spike (Q-E in §12). The
> plan's value — `CommandRegistry`, `SettingRegistry`, `FocusStack`,
> `PanelState`, redesigned settings, palette-style discoverability — does **not**
> depend on ratatui-kit. The no-r-tk plan commits to the plain-Ratatui fallback
> by default and treats the settings redesign + overall UI consistency as the
> primary deliverable. This QnA collects the specifics that plan needs from you
> before drafting.
>
> How to answer: edit this file in place under each question with your
> preference, or write "decide for me" and we'll pick a default and note it.

## How to read this

Each section is a cluster of related questions. Each question has:
- **Q** — the question.
- **Why it matters** — what part of the no-r-tk plan it pins down.
- **Options** — concrete choices with trade-offs.
- **My recommendation** — what I'd pick if you say "decide for me."

Answer as many or as few as you like; flag any as "defer" and we'll
rationalize a default in the plan.

---

## A. Overall approach & scope of the no-r-tk plan

### A1. Confirm scope: is the no-r-tk plan a *full* alternative to `ui-refactor-plan.md`, or just the *fallback* subset?

- **Why:** if full alternative, it covers everything (palette, settings, focus,
  panels, chat input, modals). If fallback subset, it covers only palette +
  settings + focus + panels and explicitly defers chat input/modals to later.
- **Options:**
  - (i) **Full alternative** — same deliverables as the r-tk path, all in plain
    Ratatui. Settings redesign + UI consistency are the headline.
  - (ii) **Fallback subset** — only what the r-tk path's "fallback column"
    names; chat-input upgrade and modal replacement are explicitly out.
- **My recommendation:** (i) **Full alternative.** The user's framing was
  "a separate plan where we don't use ratatui-kit, and instead implement the
  settings change and make the whole UI more consistent." That reads as full.

### A2. Should the no-r-tk plan replace `ui-refactor-plan.md` as the canonical plan, or coexist?

- **Why:** one canonical plan is simpler to maintain. Two plans risk drift.
- **Options:**
  - (i) **Coexist** — keep both; r-tk path stays the "if Q-E passes" branch,
    no-r-tk path is the default-and-shipped path. They share registries.
  - (ii) **No-r-tk becomes canonical** — `ui-refactor-plan.md` is archived as
    the r-tk investigation; the no-r-tk plan is the executable one.
  - (iii) **Merge into one plan** — rewrite `ui-refactor-plan.md` as a single
    plain-Ratatui plan; drop the r-tk conditional language entirely.
- **My recommendation:** (iii) **Merge** once Phase 0 answers Q-E, IF Q-E
  fails. If Q-E passes and we choose r-tk for the overlays, keep (i). Until
  then: (i) coexist.

### A3. Phase 0 (Q-E spike) — run it, or skip straight to the no-r-tk plan?

- **Why:** if the no-r-tk plan is canonical, the r-tk spike is optional cost.
- **Options:**
  - (i) **Run Phase 0 anyway** — even if no-r-tk is default, knowing whether
    r-tk could have saved bespoke work informs future phases.
  - (ii) **Skip Phase 0** — commit to no-r-tk now; revisit r-tk in 6 months
    if the bespoke popup maintenance burden is real.
- **My recommendation:** (i) **Run Phase 0 anyway.** It's a one-PR disposable
  spike and its result gates whether phases 3+ use r-tk components or plain
  Ratatui. Cheap option value.

---

## B. Command palette (no-r-tk)

### B1. Palette trigger key — `Ctrl+P` only, or multiple?

- **Why:** OpenCode uses `Ctrl+P`; some TUIs use `/`. Conflicts with existing
  `Ctrl+P`-as-no-op or terminal paste need flagging.
- **Options:**
  - (i) `Ctrl+P` only (OpenCode parity).
  - (ii) `Ctrl+P` + `Ctrl+Shift+P` (VS Code parity).
  - (iii) `Ctrl+P` + a `:palette` slash command.
- **My recommendation:** (i) `Ctrl+P`, and add `:palette` slash command as an
  alternate input. Avoids mod-shift ambiguity across terminals.

### B2. Palette layout — centered modal, top-center like VS Code, or bottom-fixed like helix?

- **Why:** affects how the palette reads against the existing top bar + chat.
- **Options:**
  - (i) Centered modal (~70% × 60%), backdrop dim. (Current `ListPopup` shape.)
  - (ii) Top strip (1 row search + 8 row results), no backdrop — helix/VS Code.
  - (iii) Bottom strip above status bar — like OpenCode's footer command menu.
- **My recommendation:** (i) **Centered modal** for v1 — reuses the existing
  `centered_rect(70, 60)` helper and matches the existing popups. (ii) is
  nicer long-term but needs new backdrop-free rendering.

### B3. Search algorithm — fuzzy (nucleo-matcher) or prefix-only?

- **Why:** fuzzy is nicer, prefix is simpler and dependency-free.
- **Options:**
  - (i) `nucleo-matcher` fuzzy over title+description+keywords+category.
  - (ii) Plain subsequence match (≤40 LOC) over title+keywords.
  - (iii) Prefix-only on title (simplest).
- **My recommendation:** (i) **nucleo-matcher** — it's already in the r-tk
  path's dependency list, well-maintained, and gives OpenCode-tier results
  matching with little code.

### B4. Blank-query behavior — curated picks, recent, or empty?

- **Why:** OpenCode shows curated commands when query is empty.
- **Options:**
  - (i) Curated picks (`chat.new`, `settings.open`, `theme.switch`, etc.).
  - (ii) Recently used commands (in-memory, last 10).
  - (iii) Hybrid: recent first, then curated.
  - (iv) Empty list with a hint line.
- **My recommendation:** (iii) **Hybrid** — recent first (so power users get
  their flow), then a curated "essential commands" block underneath.

### B5. Should palette commands also be reachable as slash commands?

- **Why:** existing slash commands (`/theme`, `/skills`, `/mcp`, `/vault`)
  already have users; removing them is friction.
- **Options:**
  - (i) Keep slash commands as-is, palette is additive.
  - (ii) Keep slash commands, palette *lists* them as commands too.
  - (iii) Migrate slash commands into palette-only, deprecate `/theme` etc.
- **My recommendation:** (ii) — palette surfaces them; slash commands stay
  as a power-user shortcut. Same as A1 of the r-tk plan §11.

### B6. Where does the palette sit in the focus stack — always-above-global, or stacked overlay?

- **Why:** always-above-global means `Ctrl+P` works inside settings. Stacked
  means closing settings first.
- **Options:**
  - (i) Always-above-global (OpenCode-like): `Ctrl+P` opens palette on top of
    whatever's focused, including settings.
  - (ii) Stacked: opening palette closes anything else first.
- **My recommendation:** (i) Always-above — matches OpenCode and feels fast.
  Inner overlay's `Esc` closes only the palette, restoring prior focus.

---

## C. Settings redesign (no-r-tk)

### C1. Replace the existing 6-tab `SettingsPopup` entirely, or layer the new searchable popup on top?

- **Why:** big-bang replace is risky; layered lets you ship browser-only first.
- **Options:**
  - (i) **Big-bang**: delete `SettingsPopup`, ship new schema-driven popup.
  - (ii) **Two PRs**: 4a browser (read-only) alongside old popup, then 4b
    editing + delete old. (Same as r-tk plan §11.)
  - (iii) **In-place retrofit**: keep the 6 tab structure, just add a search
    field across tabs.
- **My recommendation:** (ii) **Two PRs** — same split as r-tk path. Browser
  gives instant discoverability win; editing follows once the schema is
  proven. (iii) doesn't fix the underlying ad-hoc-struct problem.

### C2. Settings popup layout — paneled (left rail + main), flat list, or hybrid?

- **Why:** affects how category navigation works.
- **Options:**
  - (i) Left category rail + main setting list (r-tk plan §7.4).
  - (ii) Flat searchable list with category badges on each row; no rail.
  - (iii) Top category tabs (current) + search above.
- **My recommendation:** (i) Left rail + main list — works at 80 cols,
  search collapses the rail when active. (ii) is cleaner UX at the cost of
  list density; (iii) keeps the existing fragility.

### C3. Inline editing vs editor sub-popup?

- **Why:** inline means each row is editable in place (clean), sub-popup means
  pressing Enter opens a dedicated editor for that row.
- **Options:**
  - (i) Inline: Space toggles bool, Enter opens enum list inline, type into
    string fields in place.
  - (ii) Sub-popup: Enter opens a small centered editor for the focused row.
- **My recommendation:** (i) **Inline** for bool/enum/short-string, (ii)
  **sub-popup** for path/provider/model/keybind (longer editors). Hybrid.

### C4. Validation feedback — inline field border, toast, or footer hint?

- **Why:** needs to be obvious without breaking flow.
- **Options:**
  - (i) Red border + footer hint line.
  - (ii) Toast on invalid Enter.
  - (iii) Inline error text below the row.
- **My recommendation:** (i) Red border + footer hint — consistent with
  current `theme.error` usage.

### C5. Save model — live (write immediately on every change), commit-on-close, or commit-per-field?

- **Why:** current `SettingsPopup` snapshots on open, mirrors back on close.
  Live is more React-like but means partial states hit storage on every toggle.
- **Options:**
  - (i) Live: every change writes through `Action::SetSetting`.
  - (ii) Commit-on-close: draft state, flush on Esc-or-Ctrl+S close.
  - (iii) Commit-per-field: row gets a draft indicator, Enter commits, Esc
    reverts that row.
- **My recommendation:** (i) **Live** for bool/enum (cheap, immediate);
  (iii) **commit-per-field** for string/number/path (so a half-typed path
  doesn't get persisted). Hybrid.

### C6. Reset-to-default per row, per category, or global?

- **Why:** discoverability vs safety.
- **Options:**
  - (i) Per-row only (`d` key when row focused).
  - (ii) Per-row + "Reset this category" button.
  - (iii) Per-row + per-category + global "Reset all" (destructive confirm).
- **My recommendation:** (iii). Per-row `d`, per-category `Shift+D`, global
  reset under a destructive-confirm modal.

### C7. Advanced/dangerous settings — hidden by default, separate tab, or always visible with badge?

- **Why:** "show advanced" toggle is standard; tabs are too heavy.
- **Options:**
  - (i) Hidden by default; `a` toggles "show advanced" (a setting itself).
  - (ii) Separate "Advanced" category in the rail.
  - (iii) Always visible, danger marked with `theme.error` and requires
    confirm-on-edit.
- **My recommendation:** (i) Hidden + `a` toggles; (iii)overlay for
  destructive ones regardless of advanced-ness.

### C8. Keybindings tab fate

- **Q.** The `Keybindings` tab currently lets you cycle/preview presets. The
  new schema has no `Keybind` setting type in v1 (per r-tk plan §7.3 v1
  non-goal). What happens to the tab?
- **Options:**
  - (i) Keep as-is in the new popup, render-only (no editor).
  - (ii) Hold out of the new popup entirely; accessible via `Ctrl+K` or
    `keybinds.show` palette command, opens a dedicated keybind browser.
  - (iii) Delete for v1; keybinds are only discoverable via `keybinds.show` in
    palette (read-only list).
- **My recommendation:** (ii) Hold out as a dedicated read-only browser,
    `keybinds.show` palette command. Don't ship a half-built editor.

---

## D. Focus & modal model (no-r-tk)

### D1. `FocusStack` — host-owned single source, with or without a parallel `Overlay` trait?

- **Why:** the r-tk plan proposed both; without r-tk the `Overlay` trait is
  the only interface.
- **Options:**
  - (i) `FocusStack` + `Overlay` trait (`handle_key`/`handle_mouse`/`render`).
  - (ii) `FocusStack` only; overlays are `Option<T>` fields with bespoke
    dispatch (current shape, just made into a stack).
- **My recommendation:** (i) `Overlay` trait — uniform contract makes adding
  overlays cheap and kills the cascade.

### D2. Modal/confirm replacements — keep bespoke `QuitConfirmModal` or build a tiny `ConfirmModal` widget?

- **Why:** bespoke exists; a tiny reusable widget makes future confirms uniform.
- **Options:**
  - (i) Keep `QuitConfirmModal`, add a `ConfirmModal` widget only when a new
    confirm is needed.
  - (ii) Build a small `ConfirmModal` widget now (≤80 LOC) and migrate
    `QuitConfirm` + `delete_confirm` to it.
- **My recommendation:** (ii) Build `ConfirmModal` early — it's low-effort and
  every destructive setting (C6 reset, C7 danger) will need it.

### D3. Overlay stack ordering and `Esc` semantics — last-pushed closes, or named per-overlay?

- **Why:** last-pushed is simpler; named lets `Esc` always close a specific
  overlay even if not on top (rare).
- **Options:**
  - (i) Strict stack: `Esc` closes the top overlay only.
  - (ii) Named: `Esc` always closes "the active modal" regardless of focus
    order.
- **My recommendation:** (i) Strict stack — predictable, no surprise closes.

---

## E. Layout / panels (no-r-tk)

### E1. Confirm no mouse drag-resize — keyboard + persistent setting only?

- **Why:** parent message said "no mouse drag resize, a setting for their
  width is enough."
- **Options:**
  - (i) Keyboard + setting only (Alt+H/L, `[tui.panel]` in config).
  - (ii) Same, plus a "Drag with mouse if held Shift+Alt" escape hatch.
- **My recommendation:** (i) **Keyboard + setting only** (per your direction).

### E2. Sidebar resize granularity — 1 col or 5 col step?

- **Why:** 1 col is precise but slow; 5 col is fast but coarse.
- **Options:**
  - (i) `Alt+H/L` = 1 col, `Alt+Shift+H/L` = 5 col.
  - (ii) `Alt+H/L` = 5 col, hold for repeat.
  - (iii) `Alt+H/L` = 1 col only.
- **My recommendation:** (i) Both steps — main path. Mimics tmux/iterm
  conventions.

### E3. Auto-hide rules at small terminals — strict widths or user-overridable?

- **Why:** r-tk plan §8.4 said below 100 hides right, below 70 hides left.
- **Options:**
  - (i) Strict auto-hide at fixed widths (100/70), restore on grow.
  - (ii) Auto-hide is a hint; user can force-show with a confirm.
  - (iii) No auto-hide; user manages.
- **My recommendation:** (i) Strict auto-hide with a toast "auto-hidden due
  to terminal width; widen to restore." Predictable.

### E4. Right artifact sidebar — driven by `PanelState` like the left, or kept as today's gated `artifact_sidebar_open`?

- **Why:** symmetry vs minimal change.
- **Options:**
  - (i) Migrate to `PanelState` (width + visible) symmetric with left.
  - (ii) Keep `artifact_sidebar_open: bool` only; just expose it in settings.
- **My recommendation:** (i) Migrate — two booleans + two u16 widths in
  `PanelState` is barely more code and is consistent.

---

## F. Input fields (no-r-tk)

### F1. Chat input — keep current (`input_content`/`input_cursor`), or wrap tui-textarea in a `ChatInputState`?

- **Why:** current is load-bearing on ~15 call sites (history, click
  positioning, slash parsing). Migrating is a real PR.
- **Options:**
  - (i) Keep current for v1; render it more nicely. (r-tk plan §9.2.a.)
  - (ii) Wrap in `ChatInputState` that owns `tui_textarea::TextArea`; migrate
    all call sites in one PR. (r-tk plan §9.2.b.)
  - (iii) Keep current AND build a parallel `ChatInputState` that's
    opt-in behind a flag — switch later.
- **My recommendation:** (i) **Keep current for v1** — the focus of the
  no-r-tk plan is settings + UI consistency, not chat-input rewriting. (ii)
  is a follow-up after Phase 1 (focus) stabilizes.

### F2. Single-line inputs (palette search, settings text) — `tui-input` crate or hand-rolled?

- **Why:** `tui-input` is what r-tk uses; depends on adding the dep.
- **Options:**
  - (i) Add `tui-input`, use it for palette + settings string rows.
  - (ii) Hand-rolled `String`+`push/pop` (current pattern).
  - (iii) Hand-rolled in a small reusable `SingleLineField` widget (≤80 LOC).
- **My recommendation:** (i) **Add `tui-input`** — it has cursor/selection/
  paste that's genuinely useful and we'd otherwise re-implement. Same dep r-tk
  uses, so consistent.

### F3. External editor (`$EDITOR`) handoff — keep for artifacts only, extend to chat input?

- **Why:** current `EditorPopupState` is for artifact editing. Chat input
  handoff is a feature, not a refactor.
- **Options:**
  - (i) Keep for artifacts only in v1.
  - (ii) Add `Ctrl+E` on chat input → `EditorPopupState` with current buffer.
- **My recommendation:** (i) **Artifacts only for v1.** Chat-input editor
  handoff ships only if F1 picks (ii) and Phase 1 focus is stable.

### F4. Paste handling — bracketed-paste detection, or "treat burst of chars as paste"?

- **Why:** crossterm emits bracketed-paste events if `EnableBracketedPaste`
  is set; we don't currently enable it.
- **Options:**
  - (i) Enable bracketed paste, handle `Event::Paste` in `tui-input`.
  - (ii) Heuristic: ≥5 chars in one event loop tick = paste, fast-insert.
  - (iii) No special handling (current behavior).
- **My recommendation:** (i) **Bracketed paste** — small change, big UX win
  for path/key pastes. If F2 picks `tui-input`, it already supports it.

---

## G. Keybind discovery (no-r-tk)

### G1. Keybind help — modal popup, sticky overlay, or settings sub-page?

- **Why:** OpenCode has both a settings shortcuts tab and a `which-key`
  overlay. v1 we can afford one.
- **Options:**
  - (i) Modal popup (`?`) listing all keybinds, grouped.
  - (ii) Sticky bottom hint line that updates with focus (`help.toggle`).
  - (iii) Settings-only page behind `keybinds.show` palette command.
- **My recommendation:** (i) **Modal `?`** — matches the existing `Modal`
  pattern, doesn't claim screen real estate when closed. (iii) as a
  secondary entry from the palette.

### G2. `which-key`-style pending-sequence preview — v1 or defer?

- **Why:** OpenCode shows pending chord sequences as you type..Requires a
  prefix-key state machine.
- **Options:**
  - (i) Defer to post-v1 (r-tk plan §13 already defers `which-key`).
  - (ii) Ship a minimal version: a 1-row bottom hint showing the current
    partial chord + candidates.
- **My recommendation:** (i) **Defer** — out of v1 scope; modal `?` (G1) is
  enough.

---

## H. Reusable components (no-r-tk)

### H1. How aggressive should component extraction be in v1?

- **Why:** the more we extract, the less duplication; but each new widget is
  surface area.
- **Options:**
  - (i) Minimal: extract only what palette + settings + focus need.
  - (ii) Aggressive: build a `components/` kit (`Button`, `ToggleRow`,
    `SearchField`, `ListRow`, `Panel`, `ConfirmModal`) used everywhere
    including the existing top bar / status bar.
  - (iii) Targeted: extract `SearchField`, `ListRow`, `ToggleRow`,
    `ConfirmModal`; leave the existing host widgets untouched.
- **My recommendation:** (iii) Targeted — the four widgets cover 90% of new
  needs. Don't touch stable existing widgets in v1.

### H2. Where do shared components live — `src/ui/components/` (existing) or new `src/tui/components/`?

- **Why:** existing `src/ui/components/` has `markdown.rs`/`image_block.rs`/
  etc. (renderers); a sibling `src/tui/components/` keeps new interaction
  widgets separate.
- **Options:**
  - (i) Extend `src/ui/components/` with new widgets.
  - (ii) New `src/tui/components/` for the new overlay widgets.
- **My recommendation:** (ii) **New `src/tui/components/`** — clean
  separation, matches the r-tk plan's module layout, easy to find/maintain.

---

## I. Persistence & config (no-r-tk)

### I1. Where does `PanelState` persist — TOML `[tui.panel]` or SQLite `settings` table?

- **Why:** r-tk plan §8.5 says TOML; current `AppConfig` is TOML; existing UI
  flags like `show_chat_scrollbar` live in SQLite `settings` table per
  `ARCHITECTURE.md`.
- **Options:**
  - (i) TOML `[tui.panel]` (matches r-tk plan).
  - (ii) SQLite `settings` table (matches existing UI flags).
  - (iii) TOML for static layout, SQLite for ephemeral panels state.
- **My recommendation:** (i) **TOML** — layout is static preference, fits
  `AppConfig`'s role. Existing UI display flags already in SQLite can be
  migrated to TOML via `Setting::write` if you want them under the new
  schema's `AppConfig`-backed reads; otherwise stay in SQLite.

### I2. The schema's `read` reads from `AppConfig` (TOML) today; some settings currently live in SQLite. Reconcile how?

- **Why:** e.g. `use_env_keys`, `api_key_*` live in SQLite per
  `ARCHITECTURE.md` §3. The schema needs a single reader.
- **Options:**
  - (i) `AppSnapshot` exposes both `&AppConfig` and `&Storage` and `read`
    pulls from whichever holds the value.
  - (ii) Migrate the SQLite-keyed settings to `AppConfig` TOML (small
    migration script, additive).
  - (iii) Leave split; `Setting` declares where it lives ("config" vs
    "storage") and `read` switches on that.
- **My recommendation:** (i) **`AppSnapshot` carries both** — no migration
  risk, the schema just declares its source. (ii) is cleaner but a separate
  refactor.

### I3. Settings writable from the palette — immediate write-through, or draft-and-apply?

- **Why:** palette `theme.switch` directly applies; opening the settings popup
  for editing can be draft-y.
- **Options:**
  - (i) All settings write-through immediately (live).
  - (ii) Palette commands write-through; popup edits are draft + commit.
- **My recommendation:** (i) **All live** for v1 — simpler, matches the
  existing `apply_theme_selection` path. Drafts add a layer of state for little
  win when each setting is small.

---

## J. Slash commands & migration

### J1. Existing slash commands `/theme`, `/skills`, `/mcp`, `/vault`, `/web` — keep, augment, or hide behind palette?

- **Why:** they work today but are undiscoverable.
- **Options:**
  - (i) Keep all, palette lists them as commands.
  - (ii) Keep all, palette lists them, and add a "Type / for commands" hint at
    the chat input when empty.
  - (iii) Keep `/quit` `/exit` `/q` and deprecate the rest in favor of palette.
- **My recommendation:** (i) **Keep all, palette lists them.** Don't break
  existing users' muscle memory; palette is additive discoverability. (r-tk
  plan §13.18.)

### J2. Should new slash commands (`/settings`, `/sidebar`, `/sidebar 28`, `/theme dark`) be added?

- **Why:** power-user parity with palette.
- **Options:**
  - (i) Yes — every palette command also has a slash form.
  - (ii) No — palette is the only input; slashes stay frozen at today's set.
  - (iii) Yes for setting-style commands (`/sidebar 28`), no for action
    commands like `chat.new`.
- **My recommendation:** (iii) **Yes for setting-style** — `/theme dark`,
  `/sidebar 28` are natural; actions like `chat.new` are too short to be worth
  a slash form (just press `Ctrl+N`).

---

## K. Migration phasing (no-r-tk)

### K1. Same phase order as the r-tk plan (Phase 1 focus → 2 registries → 3 palette → 4a/4b settings → 5 panels → 6 chat input), or different?

- **Why:** r-tk plan reordered focus-first per Oracle.
- **Options:**
  - (i) Same order — FocusStack → registries → palette → settings → panels →
    chat input.
  - (ii) Different: ship settings first (the headline), then palette, then
    focus refactor.
  - (iii) Different: ship focus + palette together (they're entangled), then
    settings, then panels.
- **My recommendation:** (i) **Same order** — Oracle's reasoning holds
  regardless of r-tk. Focus routing is the substrate palette/settings key off.

### K2. Feature flag — same `--tui-v2` toggle, or per-phase flags?

- **Why:** one flag is simpler; per-phase is safer.
- **Options:**
  - (i) Single `--tui-v2` env/CLI flag, flip default after stable release.
  - (ii) Per-phase `AppConfig` booleans (`ui.palette_v2`, `ui.settings_v2`, ...).
  - (iii) No flag — ship incrementally, no rollback path.
- **My recommendation:** (i) **Single flag** — per-phase boolean soup rots
  fast. Roll back the flag in a hotfix if anything regresses.

### K3. Chat input upgrade — Phase 6 (last) or interleave earlier?

- **Why:** chat input is the highest regression risk; r-tk plan defers it.
- **Options:**
  - (i) Last (Phase 6), gated on focus Phase 1 stability — r-tk plan.
  - (ii) Interleave after Phase 4b so it doesn't bunch up at the end.
  - (iii) Skip entirely in v1; ship only if pressed.
- **My recommendation:** (iii) **Skip in v1 unless F1 picks (ii).** The
  no-r-tk plan's headline is settings + UI consistency; chat input upgrade is
  separable and high-risk. Revisit post-v1.

---

## L. Verification & rollout

### L1. How do we verify each phase before merge — manual run, snapshot tests, both?

- **Why:** TUIs are hard to test; ratatui has no snapshot harness pre-wired.
- **Options:**
  - (i) Manual `cargo run` smoke checklist per phase.
  - (ii) Add `insta` snapshot tests for popup renderings.
  - (iii) Behavioral unit tests for `CommandRegistry` / `SettingRegistry` /
    `FocusStack` only (no render snapshots).
- **My recommendation:** (iii) **Behavioral unit tests** for registries +
  focus stack (cheap, deterministic) + (i) manual smoke checklist per phase
  for visual changes. Skip (ii) snapshot tests in v1 — the existing codebase
  has none and wiring them retroactively is its own PR.

### L2. Rollback path — revert the flag, revert the PR, or feature-detect?

- **Why:** if a phase breaks for a user, we want a clean recovery.
- **Options:**
  - (i) `--tui-v2=false` env/CLI restores the old UI.
  - (ii) Each phase PR is independently revertable.
  - (iii) Both: flag for fast rollback, revert for surgical fix.
- **My recommendation:** (iii) **Both.** Flag for "this user has a problem
  right now," revert for "this change was wrong."

---

## M. Out-of-scope clarifications for the no-r-tk plan

### M1. Anything from the r-tk plan that should be **in** scope for the no-r-tk plan, beyond palette + settings?

- (i) Just palette + settings + focus + panels (fallback subset).
- (ii) Also include chat input upgrade (F1 ii).
- (iii) Also include modal consolidation (D2 ConfirmModal widget).
- (iv) Everything except r-tk itself.
- **My recommendation:** (iv) **Everything except r-tk.** The no-r-tk plan is
  a full alternative per A1; only the *widget source* differs (hand-built vs
  r-tk components).

### M2. Anything from the r-tk plan that should be **out** of scope for no-r-tk even if it was in r-tk?

- (i) `which-key` overlay (already out in r-tk v1).
- (ii) Router (`RouterProvider`) — already out.
- (iii) r-tk `Atom` global state — already out (we use `Arc<RwLock>`).
- (iv) Chat input upgrade if F1 picks (i).
- **My recommendation:** (i).* (iii) are already out. (iv) depends on F1 —
  if F1 = (i), chat input upgrade is out of no-r-tk v1 too.

---

## N. Sign-off

Once you've answered (or marked "decide for me"), I'll draft
`docs/ui-refactor_no_ratatui-kit-plan.md` using the answers as direct inputs
to each section. The r-tk plan stays as the conditional/investigation path;
the no-r-tk plan is the executable default.