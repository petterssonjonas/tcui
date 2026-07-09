# TermChatUI — ratatui-kit UX/component refactor plan

> Status: planning document, **Revision 1** (post-Oracle review + skill
> verification). No implementation in this phase.
> Scope: incremental adoption of `ratatui-kit` as a UX/component layer on top of
> the existing Ratatui frontend. Backend, provider, storage, model, MCP, and
> Obsidian logic are explicitly out of scope.
>
> **Revision 1 changelog (what changed vs the v0 draft):**
> - §2: the v0 draft misread ratatui-kit's `widget(expr)` adapter as a bridge
>   for embedding a ratatui-kit tree inside a host `Frame`. Verified against
>   the skill (`references/syntax-and-macros.md:163`: "`widget(expr)` /
>   `stateful(widget, state)` bridge any **native ratatui widget** into a
>   children position"). The bridge is native→ratatui-kit, never the reverse.
>   The skill is accurate; the v0 plan was wrong. The embedding premise is now
>   an **open question** resolved by the Phase 0 spike (see §11 + §12), with a
>   plain-Ratatui fallback that reuses the same registries.
> - §7: `Setting::write` is now `fn(Value) -> Action` (or enum), not a stored
>   `Action`. `Value` enum tightened with typed IDs.
> - §8: mouse drag-resize **removed**. Width is a persisted setting; keyboard
>   adjusts it.
> - §9: `tui-textarea` does **not** have built-in `$EDITOR` handoff — we
>   implement it via `EditorPopupState`. Chat input is now "keep v1, migrate
>   later behind a wrapper" rather than committed.
> - §10: host owns all raw crossterm events; ratatui-kit's `InputRuntime` is
>   only adopted if Phase 0 proves a supported event pump.
> - §11: Phase 0 is a binary spike; Phase 2 split (focus before palette);
>   Phase 3 split (schema+browser before editing).
> - §12 (new) collects the open questions. §13 adds the missing non-goals.
> - A separate **no-ratatui-kit** plan (`ui-refactor_no_ratatui-kit-plan.md`)
>   will be drafted after the user answers `ui-refactor-qna.md`.

This plan is grounded in:
- The current repo (`src/app/`, `src/ui/`, `src/config/`, `src/storage/`,
  `src/llm/`, `src/mcp/`, `src/obsidian/`) read from disk and via codegraph.
- The installed `ratatui-kit` skill (`/home/jp/Code/tcui/.agents/skills/ratatui-kit/`)
  and its `references/` files (`components.md`, `events-state-routing.md`,
  `hooks.md`, `syntax-and-macros.md`, `building-polished-uis.md`).
- OpenCode TUI behavior (researched from `anomalyco/opencode` source).
- The repo's own `ARCHITECTURE.md`, `DESIGN.md`, `PLAN.md`, `Cargo.toml`.

Throughout this doc, "the current app" means the on-disk TermChatUI code, and
"ratatui-kit" means the `ratatui-kit = "0.6"` crate documented in the skill.

---

## 1. Current frontend architecture

The frontend is a classic Ratatui event-loop app. There is **no component
framework** — every widget is a hand-written struct rendered into a `Frame`,
and all UI state lives in two large structs (`TuiApp` and `UI`).

### 1.1 TUI entry point and terminal setup

- `src/main.rs` — `main` (line 126): parses CLI, loads `Storage` and `AppConfig`,
  constructs `TuiApp`, and calls `app.run(&mut terminal).await`.
- `src/main.rs::setup_terminal` (line 94) — `enable_raw_mode`, `Backend::new`,
  `Terminal::new`, `EnterAlternateScreen`, `SetTitle`, `EnableMouseCapture`,
  `EnableFocusChange`. TTY-gated via `interactive_terminal_available()`.
- `src/main.rs::restore_terminal` (line 114) — inverse, plus a panic hook
  (`restore_on_drop`, line 191) so the terminal is restored on panic.
- Terminal backend type alias `crate::Backend` / `crate::TerminalType` is set
  up here. This is plain raw crossterm + ratatui 0.30.

### 1.2 The render + event loop

- `src/app/runtime.rs::run` (line 24) — the single loop. It:
  1. `terminal.draw(|f| self.ui.render(f))` every iteration (33 ms is the tick
     target, not a frame cap).
  2. `tokio::select!` over three branches:
     - `tick.tick()` — 33 ms interval; also polls `editor_popup` for `$EDITOR`
       completion (`editor.poll_output()`).
     - `reader.next()` (crossterm `EventStream`) — dispatches `Key`/`Mouse`/
       `Focus`/`Resize` to `handle_key` / `handle_mouse`. `Quit` short-circuits
       the loop.
     - `self.action_rx.recv()` — async actions from background tasks
       (streaming chunks, model refresh, connection state). Dispathed via
       `self.dispatch(action)`.
- The render loop is **synchronous and stateless per frame**: `UI::render`
  rebuilds the whole layout tree from `UI`/`ChatTabState` fields each tick.
  No widget retains internal render state across frames (except `ImageBlockState`
  and the `MarkdownRenderer` cache, both keyed by content).

### 1.3 App state shape

`TuiApp` (`src/app.rs:32`) owns:
- `storage: Storage` (SQLite)
- `config: Arc<RwLock<AppConfig>>` (TOML-backed)
- `ui: crate::ui::UI` — **all** UI state
- `vault: Option<Arc<Vault>>`
- `action_tx` / `action_rx` — unbounded tokio mpsc channel
- `system_prompt`, `ctrl_c_count`, `last_ctrl_c`, `terminal_has_focus`

`UI` (`src/ui/mod.rs:59`) is a 50+ field god struct. Highlights:
- `tabs: Vec<ChatTabState>` + `active_tab`
- `sidebar_open: bool`, `artifact_sidebar_open: bool`, `show_session_list: bool`
- `active_modal: Option<Modal>`, `focus_input: bool`
- `show_settings: bool`, `settings_popup: Option<SettingsPopup>`
- `save_file_dialog`, `export_dialog`, `artifact_viewer`, `editor_popup`,
  `list_popup` — each an `Option<…>` overlay
- a pile of `*_hit_areas` / `*_areas` fields reused for mouse hit-testing
- theming/preview flags mirrored from config: `user_alignment`, `ai_alignment`,
  `markdown_mode`, `show_selector`, `show_chat_scrollbar`, `collapse_thinking`,
  `kitty_enhanced_text`, `kitty_heading_downscale`, `image_protocol`,
  `terminal_capabilities`, `web_search_enabled`, `db_providers`,
  `visible_providers`, `current_models`, `current_reasoning_options`,
  `disabled_providers`, `disabled_models`, `frame_tick`…

`ChatTabState` (`src/ui/mod.rs:121`) holds the per-tab chat state: `messages`,
`input_content`, `input_cursor`, `input_scroll`, `input_history_index`,
`input_history_draft`, `scroll_offset`, `streaming`, dropdown open flags,
and a long list of `*_hit_area(s)` fields.

### 1.4 Action dispatch

- `src/app/action.rs::Action` — single flat enum with ~60 variants
  (`Quit`, `SendMessage`, `StreamResponse`, `ToggleSidebar`, `ShowSettings`,
  `CloseSettings`, `ToggleSettings`, `ShowSkillsPopup`, `ShowMcpPopup`,
  `ShowLocalSearch`, `MouseClick`, …).
- `src/app/runtime.rs::dispatch` (line 95) is the **only** place actions are
  matched. It is ~700 lines of `match action { … }` doing storage writes,
  config writes, LLM calls, side-effecting UI mutations, and async spawns.
- `handle_key` / `handle_mouse` return `Option<Action>`; `run` feeds it back
  into `dispatch`. Background tasks send `Action::*` through `action_tx`.

This is a clean separation in principle, but in practice `dispatch` is a
monolith that mixes UI plumbing (`self.ui.show_settings = …`) with persistence
and LLM work. There is no concept of an action **category** or source.

### 1.5 Keyboard handling

All keyboard handling lives in `src/app/input_events.rs::handle_key` (line 6),
a 600+ line method. The dispatch order is a **hard-coded if/else cascade over
`self.ui.*` overlay flags**:

1. `export_dialog` open → handle there, return
2. `Ctrl+E` → `ExportConversation` (global shortcut buried inside the cascade)
3. `save_file_dialog` open → handle, return
4. `editor_popup` open → `editor.handle_key`, return
5. `artifact_viewer` open → handle, return
6. `list_popup` open → handle, return
7. `active_modal` (quit confirm) → handle `y/n/Esc`, return
8. `delete_confirm` → handle, return
9. `show_settings` → settings tab-specific huge match (Tab/ arrows/ Enter/ Space,
   with per-tab dropdown sub-branches for `general_dropdown_open`,
   `providers_dropdown_open`, `models_dropdown_open`, `preset_key_popup`),
   return
10. else → "normal" chat mode: `Ctrl+B` sidebar, `Ctrl+]` artifact sidebar,
    `Ctrl+T` new tab, `Ctrl+N` new chat, `Ctrl+W` close tab, `Ctrl+,`/`Ctrl+S`
    settings, `Ctrl+R` refresh, arrows for scroll/history, Enter for send with
    inline slash-command parsing (`/quit`, `/skills`, `/mcp`, `/web`,
    `/theme`, `/vault`).

**What is fragile here:**
- The overlay priority is encoded as a nested `if` chain in source order. Adding
  a new overlay means re-threading the chain. There is no overlay stack — each
  popup is a separate `Option<…>` on `UI`, and the code must remember to check
  every one in the right order.
- The settings branch (step 9) is ~250 lines of per-tab special cases. Each
  tab+dropdown combination has its own up/down/enter behavior. There is no
  abstraction for "a settings row" or "a dropdown".
- Slash commands (`/theme`, `/web on`, `/vault …`) are parsed inside the Enter
  handler with `strip_prefix` chains. They are undiscoverable and inconsistent
  with the keybind system.
- Hit-area fields (`provider_hit_area`, `model_hit_area`, `*_hit_areas`) are
  written during `render` and read during `handle_mouse`. The link between a
  hit area and its handler is implicit and easy to break.
- `Ctrl+S` is overloaded to both "save file dialog confirm" and "toggle settings"
  depending on which overlay is open, purely by virtue of cascade ordering.

### 1.6 Mouse handling

`src/app/input_events.rs::handle_mouse` (line 616) and `handle_mouse_click`
(line 814) walk the same pile of `*_hit_area` fields. Click handling is a flat
cascade: chat scrollbar, sidebar, tabs, status bar provider/model dropdowns,
thinking toggles, links, artifact sidebar. No event target abstraction.

### 1.7 Modal / overlay handling

There is no modal stack. Each overlay is an `Option<…>` on `UI`:
- `Modal` enum (`src/ui/mod.rs:117`) — currently only `QuitConfirm`.
- `QuitConfirmModal` (`src/ui/modals/quit_confirm.rs`) — bespoke centered box
  with `[Y]es` / `[N]o` hit areas, hardcoded key handling (`y`/`n`/`Esc`).
- `delete_confirm: Option<ArtifactHandle>` — reuses `QuitConfirmModal` with a
  different title/message.
- `save_file_dialog: Option<SaveFileDialog>` and `export_dialog` — bespoke
  overlays with their own `focus` enum and `cycle_focus`.
- `artifact_viewer`, `editor_popup`, `list_popup` — each its own bespoke popup.

Each overlay re-implements: `Clear` the area, draw a `Block` with `bg=black`,
compute a centered rect, render content, return hit areas. There is no shared
`Modal`/`Popup` primitive in the codebase; the only thing they share is the
`centered_rect(percent_x, percent_y, area)` helper pattern, which is duplicated.

### 1.8 Settings UI

`SettingsPopup` (`src/ui/settings_tab/mod.rs:43`) is a ~60-field struct
mirroring the entire editable config surface:
`default_provider`, `default_model`, `small_model`, `theme`, `user_alignment`,
`ai_alignment`, `markdown_mode`, `artifact_save_dir`, `vault_path`,
`show_selector`, `show_chat_scrollbar`, `collapse_thinking`,
`kitty_enhanced_text`, `kitty_heading_downscale`, `web_search_enabled`,
`quit_confirmation`, `local_enabled`, `local_host`, `local_port`,
`local_server_type`, `local_selected_model`, … plus per-tab focus enums
(`GeneralFocus`, `LocalFocus`, `ProvidersTabFocus`, `ModelsTabFocus`),
per-tab dropdown states (`general_dropdown_open`, `providers_dropdown_open`,
`models_dropdown_open`, `preset_key_popup`), and hit-area structs.

Six hardcoded tabs (`SettingsTab`: `General`, `Keybindings`, `Providers`,
`Models`, `Local`, `Mcp`). Rendering is `match self.active_tab { … }` calling
`render_general`, `render_keybindings`, `render_providers`, `render_models`,
`render_local`, `render_mcp`. Each is a bespoke function returning hit areas.
Editing is character-at-a-time: `settings.type_char(c)`, `settings.backspace()`,
with per-field `String` buffers (`local_host`, `local_port`, `vault_path`, …).
There is no setting schema, no validation, no "reset to default", no search.

Open/save flow (`src/app/runtime.rs` `ShowSettings`/`CloseSettings`/`ToggleSettings`):
on open, `load_settings_popup_state` snapshots `AppConfig` + storage into
`SettingsPopup`; on close, `save_settings_popup_state` writes back. Several
`UI` flags are then manually re-mirrored (`user_alignment`, `markdown_mode`, …)
in both `CloseSettings` and `ToggleSettings` — duplicated logic.

**Inconsistency:** settings uses a tab+focus+dropdown state machine hand-rolled
in input_events.rs. Every other overlay (skills, mcp, themes, local search)
reuses the generic `ListPopup` instead. So there are two parallel popup systems.

### 1.9 Input handling

Chat input is hand-rolled in `src/app/input.rs`. The input "field" is just two
fields on `ChatTabState` (`input_content: String`, `input_cursor: usize`).
Operations (`insert_input_char`, `backspace_input_char`, `delete_input_char`,
`move_input_cursor_*`, `set_input_cursor_from_click`, `insert_input_text`,
`replace_input_content`, `browse_input_history`) mutate those two fields and
call `refresh_input_popup()`. Multiline is supported only insofar as
`input_content` can contain `\n`; cursor/scroll logic is char-index based via
`char_to_byte_index`. There is no selection, no paste detection (paste looks
like a burst of `Char` events), no `$EDITOR` handoff for the chat input
(`editor_popup` is for artifact editing only, via `EditorPopupState`).

The single-line popups (palette search, save path, export path, settings text
fields) all re-implement `push(c)` / `pop()` on local `String` buffers. There
is no shared input widget. The `tui-input` and `tui-textarea` crates are
**not** in `Cargo.toml`.

Command history (`input_history_index`, `input_history_draft`) is per-tab,
Up/Down browses it. History is in-memory only; not persisted.

### 1.10 Sidebar / layout code

`src/ui/mod.rs::render` (line 283) builds the layout each frame:
```
Vertical: [Length(1) top bar, Min(0) body, Length(1) status bar]
Body → 3 rects: [left sidebar, chat area, right artifact sidebar]
```
Sidebar widths are constants: `sidebar::SIDEBAR_WIDTH` (24, per the design doc),
artifact sidebar `min(max_artifact_width, 32)` with `show_artifact_sidebar`
gated on `main_layout[1].width - left_width >= 72`. The artifact sidebar
collapses to width 0 below that threshold. There is **no** resizing, no
persistence of panel sizes, no min/max config, no right-sidebar-for-inspector.
`sidebar_open` and `artifact_sidebar_open` are booleans toggled by `Ctrl+B`
and `Ctrl+]`.

### 1.11 Page / view structure

There are no "pages" — the app is one screen with overlays. The closest thing
to a page is "settings open vs closed" and the per-tab chat. Sessions/list,
themes, skills, mcp, local search are all `ListPopup` overlays. There is no
router, no route history, no deep-linking.

### 1.12 Reusable widgets / components

`src/ui/components/`:
- `markdown_model.rs` — `MarkdownRenderer` with kitty/heading/image awareness.
  Specialized, not ratatui-kit-able.
- `image_block.rs` — `ImageBlockState` for `ratatui-image`.
- `terminal_capabilities.rs` — feature detection.
- `chat_message.rs` — a basic `ChatMessage` bubble; barely used (chat_tab
  has its own rendering).
- `collapsible.rs` — collapsible thinking blocks.

Sidebar/top_bar/status_bar/tab_bar/toast/session_list are each their own file
with a bespoke `render`. There is no shared "list row", "search field",
"button", "panel" component.

### 1.13 Duplicated / ad hoc UI state

- Theming/preview flags are mirrored in **four** places: `AppConfig`,
  `UI`, `SettingsPopup`, and `ChatTabProps` (which re-receives them every
  frame). Keeping these in sync is manual (`CloseSettings` copies a fixed list).
- Per-tab focus enums + dropdown states in `SettingsPopup` duplicate the
  "row list + active index" pattern that `ListPopup` already implements.
- `centered_rect` is implemented in `quit_confirm.rs`, `settings_tab/mod.rs`,
  `artifact_viewer.rs`, and `list_popup.rs` independently.
- The `ListPopup` and `SettingsPopup` are two parallel overlay systems.
- Hit-area fields are written in render and read in mouse handlers scattered
  across `input_events.rs`, with no schema tying a hit area to an action.

### 1.14 What currently feels inconsistent or fragile (summary)

1. **Overlay dispatch is positional `if` chains** in one 600-line file. Adding
   a popup means re-threading priority; there is no overlay stack.
2. **Settings is a parallel universe** — its own editor primitives, its own
   focus enums, its own dropdown state, its own key handling. It does not reuse
   `ListPopup` or any shared input.
3. **No setting schema** — fields are ad hoc structs; no validation, no reset,
   no search, no docstring/description, no "requires restart" flag.
4. **Input is a pair of fields hand-edited char by char** — no multiline
   awareness, no selection, no paste, no `$EDITOR` for the chat input, no
   shared component for the 5+ single-line inputs.
5. **State is mirrored 4×** and re-synced manually on settings close.
6. **No component reuse** — `centered_rect`, list row, popup frame, and button
   are re-implemented per file.
7. **Slash commands are parsed in the Enter handler** with `strip_prefix`;
   they are undiscoverable and inconsistent with keybinds.
8. **No router / no pages** — settings and popups are boolean flags, not
   navigable destinations.
9. **Mouse hit areas are implicit** — render writes, mouse reads, the link is
   convention, not type.

---

## 2. ratatui-kit fit analysis

ratatui-kit (v0.6) is a React-like component framework layered on Ratatui +
Tokio. Per the skill (`SKILL.md` + `references/`), its strengths are:
declarative `element!` trees, `#[component]` function components,
`use_state`/`use_atom` reactive state, `use_event_handler` with input layers
and priorities, `RouterProvider`/`routes!`, and a set of built-in components
(View, Border, Center, Text, WrappedText, Modal, ConfirmModal, AlertModal,
ShortcutInfoModal, Select, MultiSelect, ScrollView, ContextProvider,
Positioned, Fragment) plus feature-gated `Input`/`SearchInput` (input),
`TreeSelect` (tree), `VirtualList` (virtual-list). The `textarea` feature is
**offline** in 0.6 (its dep doesn't build against Ratatui 0.30) — do not rely
on a built-in textarea.

Critically, ratatui-kit is a **render-time framework that owns its own loop**
(`element!(App).fullscreen().await`; `references/syntax-and-macros.md:320`,
`SKILL.md:160`). It runs a central `InputRuntime` that calls `begin_frame` each
frame (`references/events-state-routing.md:1,14`). TermChatUI's current loop
(`TuiApp::run` + `UI::render`) directly consumes crossterm events itself.

The v0 draft assumed ratatui-kit could be embedded "as a guest inside an
existing `Frame` draw via the `widget()` adapter." **That is wrong.** Per
`references/syntax-and-macros.md:163`, `widget(expr)` / `stateful(widget, state)`
bridge a **native Ratatui widget into an `element!` tree** — the direction is
native→ratatui-kit, never the reverse. The skill documents no public API to
render a ratatui-kit `element!` tree into a host-provided `Frame`/sub-rect, nor
to adopt the `InputRuntime` piecemeal inside an existing loop.

Therefore the **central open question** for this plan is binary:

- **Q-E.** Does ratatui-kit 0.6 expose a public, non-`fullscreen()` API to
  (a) render an `element!`/`AnyElement` tree into a caller-provided
  `Frame`/`Buffer`/`Rect`, AND (b) pump crossterm events into its `InputRuntime`
  from outside? If yes → "ratatui-kit islands inside the host" is viable.
- If **no** (the skill strongly suggests no) → fall back to **plain Ratatui**
  palette/settings built from the same `CommandRegistry` / `SettingRegistry` /
  `FocusStack`. Those registries are the load-bearing substrate and survive
  either path.

**Phase 0 (§11) is a disposable spike that answers Q-E.** Until Q-E is
answered, every "use ratatui-kit" entry below is conditional on the spike
passing; the fallback column shows the plain-Ratatui work that happens if it
fails.

### Area-by-area recommendation

"r-tk" = ratatui-kit. "Host" = existing `TuiApp::run` + `UI::render`. Rows
marked **conditional** are only "use r-tk" **if Phase 0 spike (Q-E) passes**;
the **fallback** column is the plain-Ratatui work that ships either way, because
the registries and `FocusStack` are framework-agnostic.

| Area | If Q-E passes | Fallback (or always) | Notes |
|---|---|---|---|
| Top-level app shell / render loop | **keep plain Ratatui** | always | ratatui-kit's `fullscreen()` owns the loop; we never move the root into it. |
| Command palette | **use r-tk** `Modal`+`SearchInput`+`Select`+input layer | plain-Ratatui popup that reads `CommandRegistry` + a `tui-input` field + a hand `StatefulList` | palette is a self-contained popup; both paths share `CommandRegistry`. |
| Settings popup (redesign) | **use r-tk** `Modal`+`Select`/`MultiSelect`+per-row `Input` | plain-Ratatui popup driven by `SettingRegistry` + a reusable `SearchField` + `StatefulList` rows | `SettingRegistry` is the value; r-tk only saves bespoke rendering. |
| Searchable lists (sessions, themes, models, providers, skills, mcp, vault) | **use r-tk** `Select`/`VirtualList` (large lists) | always keep `ListPopup` for tiny transient popups | `VirtualList` only matters for conversations (potentially huge); others are fine with a plain `StatefulList`. |
| Settings boolean toggle | **use r-tk** `MultiSelect` (single) | a tiny custom `ToggleRow` widget (≤30 LOC) | both small. |
| Settings enum / dropdown | **use r-tk** `Select` | plain-Ratatui `StatefulList` popover | |
| Settings string / path / number fields | **use r-tk** `Input`/`SearchInput` | `tui-input` crate wrapped in a small host widget | `tui-input` is what r-tk's `input` feature is built on; the fallback is the same widget without r-tk on top. |
| Keybind capture / editing | **defer** bespoke, host-owned | always | no built-in keybind editor in r-tk; this is custom either path. v1 non-goal (keep current `Keybindings` tab). |
| Multiline chat input | **keep custom + adopt `tui-textarea`** (see §9) | always (r-tk's `textarea` feature is offline in 0.6) | `tui-textarea` is a native Ratatui widget both paths use. |
| Sidebar / panel layout | **keep plain Ratatui** `Layout` with `Constraint` | always | adding a flex engine buys nothing here. Add `PanelState` (§8). |
| Resizable panels | **small host helper, not r-tk** | always — **keyboard only, no mouse drag** | width is a persisted setting; Alt+H/L adjusts. See §8. |
| Focus / modal stack | **host-owned `FocusStack`** + r-tk input layers per active island (if Q-E) | host-owned `FocusStack`; overlays expose host-level `handle_key`/`handle_mouse` contracts | the `FocusStack` is the load-bearing piece; r-tk layers are an optimization. See §10. |
| Global state (theme, config snapshot, panel sizes, keymap) | r-tk `Atom` inside islands for pure-UI state | always: plain `Arc<RwLock<…>>` for the host; `AppConfig` never becomes an atom | don't migrate `AppConfig` to atoms. |
| Routing | **defer** r-tk router | always | app is single-screen; router adds complexity for no gain. |
| Markdown / chat transcript rendering | **keep plain Ratatui** | always | `MarkdownRenderer` is specialized (kitty headings, images, anchors, diff). |
| Image rendering | **keep plain Ratatui** (`ratatui-image`) | always | native widget. |
| Status bar / top bar / tab bar | **keep plain Ratatui** for v1 | always | dense, stable; no payoff from re-templating. |
| Toast / transient messages | **keep plain Ratatui** (`src/ui/toast.rs`) | always | one-frame overlay. |
| Theming | **keep `crate::theme`** | always | r-tk has no theme system. |
| Async side effects (model refresh, connection check) | **keep current `tokio::spawn` + `action_tx`** | always | r-tk's `use_future` is per-component; TermChatUI's async is app-lifetime. |

### Risks

- **Q-E blocker (CRITICAL).** The entire "ratatui-kit island" premise hinges on
  a public host-render + event-pump API that the skill does not document. If
  the spike proves no such API exists, every "use r-tk" row above collapses to
  its fallback column. **Mitigation:** all conditional rows have a concrete
  plain-Ratatui fallback that shares the same registry substrate; the
  refactor's value (palette, settings schema, focus, panel sizes) is delivered
  either way. See §11 Phase 0 + §12.
- **State ownership conflicts (HIGH) — only if Q-E passes.** ratatui-kit
  islands own reactive state (`use_state`), but TermChatUI's source of truth is
  `TuiApp`. An island must read/write through a narrow bridge: dispatch
  `Action`s up and read a snapshot down. Naive `use_state` inside an island
  that duplicates `UI` fields will desync. **Mitigation:** islands are
  stateless renderers of a passed-in `&` snapshot + a `Handler<Action>`; they
  emit `Action`s, never mutate `UI`.
- **Async / runtime conflicts (MEDIUM — only if Q-E passes).** ratatui-kit
  uses Tokio and runs its own redraw when reactive state changes. Inside a
  host-rendered island, the host must drive redraws at 33 ms anyway, so
  `use_future` wakeups re-render into the host's next frame. The spike must
  confirm a host-rendered r-tk subtree actually re-renders on `State::set`
  without the host calling `fullscreen()`. If not, fall back to "host redraw
  on tick only" (still fine — the host already ticks at 33 ms).
- **Lifetime / borrow complexity (MEDIUM).** The current `ChatTab<'a>` borrows
  `&'a mut ChatTabState`. ratatui-kit components prefer `'static` props
  (`AnyElement<'static>`, `Handler<'static>`). Islands will need to **clone**
  snapshot data into props each frame, not borrow. This is a real cost for the
  chat transcript; that's why chat transcript rendering stays plain Ratatui.
- **Event routing conflicts (HIGH).** The host already has a cascade dispatch.
  If a ratatui-kit island registers `use_event_handler`, both the host and the
  island will see the same crossterm event and may double-handle. **Mitigation:**
  the host must short-circuit its `handle_key` cascade for keys owned by an
  active island, and the island must use an input layer (`blocks_lower=true`)
  to capture its own keys. This needs a clean contract (§10).
- **Focus bugs (MEDIUM).** With islands + host overlays + a palette, focus is
  split across three systems. Without a single focus-stack owner, you get
  classic "palette ate my `j`" or "background scrolled behind the modal" bugs.
  Needs §10's focus manager.
- **Performance / render cost (LOW–MEDIUM).** ratatui-kit reconciles a tree
  each frame. For a small palette/settings island this is negligible. For the
  full chat transcript it could double render time. That's another reason chat
  stays plain Ratatui.
- **Migration risk (MEDIUM).** ratatui-kit is edition 2024; TermChatUI is
  edition 2021. They can coexist in one binary, but a new workspace member or
  a Cargo feature dance may be needed. Phase 0 verifies this.

---

## 3. Where ratatui-kit should NOT be used

Stay plain Ratatui or existing code, **at least for now**:

1. **`src/main.rs` terminal setup** — `setup_terminal`, `restore_terminal`,
   `restore_on_drop`. This is crossterm raw mode + alternate screen. Keep.
2. **`src/app/runtime.rs::run` event loop** — keep the `tokio::select!` loop,
   the `action_rx` channel, the tick. This is the host.
3. **`src/app/action.rs::Action` enum and `dispatch`** — the action bus stays.
   ratatui-kit islands emit `Action`s into the existing `action_tx`; they do
   not replace it.
4. **`src/app/input_events.rs::handle_key`/`handle_mouse`** for the **host
   shell** — top bar, sidebar, status bar, chat area, mouse hit testing of
   those. (The palette and settings become ratatui-kit islands; the host stops
   dispatching to them, instead routing to the island via an input layer.)
5. **`src/ui/chat_tab.rs` chat transcript rendering** — `RenderedMessages`,
   thinking toggles, kitty headings, image states, anchors, diff view. Too
   specialized; ratatui-kit would add friction and lifetime pain.
6. **`src/ui/components/markdown_model.rs`** — `MarkdownRenderer` stays.
7. **`src/ui/components/image_block.rs` and `ratatui-image` integration** —
   native widget, bridge only if needed.
8. **`src/llm/`, `src/mcp/`, `src/obsidian/`, `src/storage/`, `src/config/`**
   — entirely out of scope. ratatui-kit touches none of these.
9. **`src/ui/toast.rs`** — transient host-owned overlay, not reactive.
10. **`src/ui/top_bar.rs`, `src/ui/tab_bar.rs`, `src/ui/status_bar.rs`,
    `src/ui/sidebar.rs`, `src/ui/artifact_sidebar.rs`** — keep plain Ratatui
    for v1. They are dense, stable, and re-templating them yields little. The
    *only* change to these in scope is: (a) sidebar widths become driven by the
    new `PanelState` (§8), and (b) sidebar/session list *contents* can later be
    upgraded to a ratatui-kit `Select`/`VirtualList` island if desired — but
    that is **deferred**, not in this plan.
11. **Routing** — ratatui-kit's `RouterProvider` is **not** used. The app is
    single-screen with overlays; a router adds complexity for no current gain.
12. **`textarea` feature of ratatui-kit** — offline in 0.6. Use `tui-textarea`
    directly (§9) if we adopt a real multiline widget.

---

## 4. OpenCode-inspired UX model

### 4.1 What OpenCode actually does (from source research)

- **Command palette** (`command.palette.show`, default `ctrl+p` in the TUI):
  a fuzzy filter (`fuzzysort`) over `display`/`category`/`description`/`keywords`.
  Blank query shows curated picks (`session.new`, `workspace.new`, …). Esc
  closes; Up/Down or Ctrl+P/Ctrl+N navigates; Enter selects. The palette
  merges **commands + files + sessions** in one `DialogSelectFile`.
- **Settings** is a **tabbed modal** (General / Shortcuts / Servers / Providers
  / Models), not a single searchable page. Search is **tab-local** (e.g.
  Models tab has a filter, Shortcuts tab has a fuzzy list). **There is no
  global settings search in OpenCode.**
- **Keybind discovery** has two surfaces: Settings → Shortcuts, and a
  `which-key` overlay/dock with live pending-sequence preview.
- **Theme/model/provider** are commands in the palette *and* dedicated popovers
  (composer model selector, Settings tabs).
- **Layout** in the TUI is a split-footer architecture (scrollback immutable,
  footer repaintable). App shell persists `sidebar.width`, `terminal.height`,
  etc. No drag-resize in the CLI TUI panels inspected.
- **Persistent TUI config** lives in `tui.json` (theme, keybinds,
  `leader_timeout`, `attention`, `prompt`, `scroll_speed`, `diff_style`,
  `mouse`, plugin state).
- **Modal/focus** uses a `DialogProvider` stack (`show()` clears, `push()`
  stacks; Escape closes top; close has a settle timeout). Keymap layers are
  **mode-based** with priority (which-key `priority: 1000`, footer subagent
  `priority: 1`).
- **Input**: chat is a `contenteditable` div (web); palette/settings use
  `TextField`/`SelectV2`; external editor handoff via `openEditor()` with a
  temp file.

### 4.2 What to adopt in TermChatUI

TermChatUI is a chat workspace, not a coding agent. We borrow:

1. **`Ctrl+P` command palette** as the primary fast entry point.
2. **Searchable commands** with fuzzy match over `title`/`description`/
   `keywords`/`category`, with curated "blank query" picks.
3. **Searchable settings popup** as a deeper editor (separate from, but
   launched by, the palette). OpenCode lacks global settings search — we **add
   it**, because our settings are denser (provider+model+local+MCP+Obsidian).
4. **Provider/model/session actions** in the palette (currently scattered as
   status-bar dropdowns and slash commands).
5. **Theme selector** in the palette (currently `/theme` or Settings).
6. **Keybind discovery** as both a palette command (`Open keybinds`) and a
   `which-key`-style overlay later (deferred to post-v1).
7. **Layout commands** (toggle/resize sidebar, reset layout) in the palette.
8. **MCP/Obsidian/local inference commands** in the palette — currently ad hoc
   `/mcp`, `/vault` slash commands.
9. **Help** in the palette.

### 4.3 A vs B vs C — recommendation

- **A (settings only on a dedicated screen):** loses fast access.
- **B (settings only under palette):** good for booleans/enums, bad for
  multi-field forms (add provider: name + endpoint + key + presets).
- **C (palette as fast entry, searchable settings popup for deep editing):**
  best fit.

**Recommendation: C**, with one refinement informed by the repo:
the settings popup is a **searchable categorised popup** (not the current
6 hard tabs). The palette is the *only* fast entry point; the popup is for
browsing/filtering and for edits that need more than a toggle.

We **challenge** the user's preference only on one point: do **not** make the
popup a full "page" with routing. Keep it a modal overlay. The app is
single-screen; a route adds machinery for no gain. The popup already has
`popup_area = centered_rect(70, 80, …)`; we redesign its **contents**, not its
placement.

---

## 5. Target UI architecture

A practical module layout. New modules are added **gradually** (see §11);
existing files move only when their replacement is ready.

```text
src/tui/                       # NEW, island + UX layer
  app_bridge.rs                # host↔island bridge: Action bus, snapshot import
  focus/
    manager.rs                 # FocusStack + Focus enum + event priority
  palette/
    registry.rs                # CommandRegistry: id, title, category, keywords, action
    command.rs                 # Command value type
    palette.rs                 # ratatui-kit island: Modal + SearchInput + Select
  settings/
    schema.rs                  # Setting, SettingType, SettingCategory
    registry.rs                # SettingRegistry: all settings metadata
    popup.rs                   # ratatui-kit island: searchable settings popup
    editors/                   # per-type editor components
      bool.rs
      enum.rs
      string.rs
      path.rs
      keybind.rs
      provider_selector.rs
      model_selector.rs
  layout/
    panels.rs                  # PanelState { sidebar_left, sidebar_right, … }
    resize.rs                  # resize policy + persistence
  theme/
    bridge.rs                  # bind crate::theme::Theme -> ratatui-kit Style
  input/
    chat_input.rs              # thin wrapper over tui-textarea (see §9)
    single_line.rs             # SearchInput/host single-line field helper
  components/                  # small reusable islands (built incrementally)
    list_row.rs
    button.rs
    panel.rs

src/ui/                        # EXISTING host rendering (kept)
  mod.rs                       # UI struct slimmed: drops settings_popup, list_popup_phase2
  chat_tab.rs                  # chat transcript (kept, plain Ratatui)
  sidebar.rs, top_bar.rs, status_bar.rs, tab_bar.rs # kept
  modals/                      # kept; QuitConfirm later swapped for ratatui-kit ConfirmModal
  settings_tab/                # Deprecated and removed in Phase 3
src/app/                       # EXISTING app layer (kept)
  action.rs                    # Action enum extended, not replaced
  runtime.rs                   # run loop: now also pumps palette/settings islands
  input_events.rs              # host key dispatch slimmed; overlays delegated
```

### Per-module responsibilities

**`src/tui/app_bridge.rs`**
- *Responsibility:* the contract between host (`TuiApp`/`UI`) and islands.
- *Important types:* `AppSnapshot<'a>` (read-only view into `UI` + `AppConfig`
  a palette/settings island needs), `ActionSink` (wraps `action_tx`),
  `IslandProps<'a>` (props bundle for islands: snapshot + sink).
- *State ownership:* **none**. Pure bridge. Host calls
  `render_island(&mut frame, area, &snapshot, &sink)` per frame for each active
  island.
- *Receives app state:* `&UI` + `&AppConfig` reference per frame.
- *Emits app actions:* `ActionSink::send(Action)` -> `action_tx`.
- *Files reused:* none new. Uses `crate::app::Action`.

**`src/tui/focus/manager.rs`**
- *Responsibility:* single source of truth for "what is on top" and event routing.
- *Important types:* `FocusStack`, `enum Focus { Chat, Sidebar, Palette,
  Settings, Modal(ModalId), EditorPopup }`.
- *State ownership:* lives on `UI` as `focus: FocusStack` (replaces
  `active_modal`, `show_settings`, `list_popup`, etc. as a stack).
- *Receives app state:* mutated by host on open/close; read by `handle_key` to
  decide dispatch.
- *Emits app actions:* none directly; `Focus` transitions trigger `Action`s in
  the host (e.g. opening palette pushes `Focus::Palette` and emits
  `Action::OpenPalette` which the host uses to register the island).
- *Files reused:* absorbs `UI::active_modal`, `UI::show_settings`,
  `UI::list_popup`, `UI::save_file_dialog`/`export_dialog` flags. Existing
  bespoke popups migrate gradually.

**`src/tui/palette/registry.rs` + `command.rs`**
- *Responsibility:* the canonical command list.
- *Important types:* `Command { id, title, description, category, keywords,
  shortcut: Option<KeyEvent>, enabled: fn(&AppSnapshot)->bool, action: Action,
  preview: Option<PreviewFn>, setting_target: Option<SettingId> }`,
  `CommandRegistry` (built at startup, queried with a fuzzy match).
- *State ownership:* `CommandRegistry` is `OnceLock<CommandRegistry>` built once
  in `main`; not reactive.
- *Receives app state:* `enabled` predicates take `&AppSnapshot`.
- *Emits app actions:* the `action` field is fired on Enter.
- *Files reused:* reuses the existing `Action` enum; each command maps to one
  existing action. e.g. `ToggleSidebar` -> `Action::ToggleSidebar`.

**`src/tui/palette/palette.rs`** (ratatui-kit island)
- *Responsibility:* render the palette and handle its keys.
- *Important types:* `PaletteProps<'a> { snapshot, sink, open: bool }`.
- *State ownership:* **ephemeral in-island** `use_state` for the search string
  and selected index; cleared on close.
- *Receives app state:* through `PaletteProps`.
- *Emits app actions:* on Enter, `sink.send(command.action)`.
- *Files reused:* none; replaces the `/theme`/`/skills`/`/mcp`/`/vault`
  slash-command paths and the `ListPopup` usages for those.

**`src/tui/settings/schema.rs` + `registry.rs`**
- *Responsibility:* declarative settings metadata + runtime value accessors.
- *Important types:*
  ```rust
  pub struct Setting {
      pub id: SettingId,
      pub title: &'static str,
      pub description: &'static str,
      pub category: SettingCategory, // Providers, Models, Local, Themes, Keybinds, Mcp, Obsidian, Layout, Chat, Privacy, Experimental
      pub keywords: &'static [&'static str],
      pub value_type: SettingType,    // Bool, Enum(&[&str]), String, Number, Path, Keybind, ProviderSelector, ModelSelector
      pub default: DefaultValue,
      pub read: fn(&AppSnapshot) -> CurrentValue,
      pub write: SettingWrite,        // see §7.2 — produces the action, takes the edited value
      pub requires_restart: bool,
      pub advanced: bool,
      pub danger: Dangerous,          // None, Caution, Destructive
  }
  ```
- *State ownership:* `SettingRegistry` built once at startup.
- *Receives app state:* `read` closures over `&AppSnapshot`.
- *Emits app actions:* `write` produces an `Action` from the edited `Value`
  on edit. (v1 non-goal: keybind capture — stays in the `Keybindings` tab.)
- *Files reused:* replaces `SettingsPopup`'s 60 ad-hoc fields with declarative
  metadata; the existing `Action::ToggleWebSearch`, `Action::SaveApiKey`, etc.
  become specific `SettingWrite::Once(action)` variants; most settings use
  `SettingWrite::Generic` → `Action::SetSetting(SettingId, Value)`.

**`src/tui/settings/popup.rs`** (ratatui-kit island)
- *Responsibility:* searchable, categorised settings editor.
- *Important types:* `SettingsPopupProps<'a> { snapshot, sink, open }`,
  in-island `use_state` for search, category filter, selected setting, draft
  value.
- *State ownership:* draft values are local; committed via `write` action.
- *Receives app state:* `AppSnapshot`.
- *Emits app actions:* `Action::SetSetting(...)` or specific actions.
- *Files reused:* reuses the *read* paths from `src/app/settings.rs`
  (`load_settings_popup_state` becomes `AppSnapshot::settings_view()`); the
  bespoke `SettingsPopup` struct is deleted.

**`src/tui/layout/panels.rs` + `resize.rs`**
- *Responsibility:* panel widths + persistence.
- *Important types:*
  ```rust
  pub struct PanelSizes { pub sidebar_left: u16, pub sidebar_right: u16,
                          pub input_height: u16, pub status_bar: u16 }
  ```
  persisted next to `AppConfig` (small addition; see §13 non-goals). Resizing
  via `Alt+H/L` (sidebar width ±1), `Alt+Shift+H/L` (jump by 5), `Alt+0` reset.
- *State ownership:* on `UI` as `panel_sizes: PanelSizes`, atom-equivalent for
  islands that care.
- *Receives app state:* host reads it in `UI::render` to compute `content_layout`.
- *Emits app actions:* `Action::ResizeSidebar{delta}`, `Action::ResetLayout`.
- *Files reused:* `src/ui/mod.rs::render` body where `SIDEBAR_WIDTH` constant
  is used; the `artifact_width` math becomes a `PanelSizes`-driven calc.

**`src/tui/theme/bridge.rs`**
- *Responsibility:* map `crate::theme::Theme` colors into `ratatui::style::Style`
  for islands. No theme engine change.
- *Files reused:* `src/theme.rs`.

**`src/tui/input/chat_input.rs` + `single_line.rs`**
- *Responsibility:* input widget wrappers (see §9).
- *Files reused:* `src/app/input.rs` (chat) and the per-overlay `String` inputs.

**`src/tui/components/`**
- *Responsibility:* small reusable islands (list row, button, panel frame).
- Built incrementally; only what's needed by palette/settings.

### What moves where (concrete)

| Existing | Destination | When |
|---|---|---|
| `UI::show_settings`, `UI::settings_popup`, `SettingsPopup` struct | `FocusStack` + `src/tui/settings/popup.rs` + `src/tui/settings/schema.rs` | Phase 3 |
| `UI::list_popup` for theme/skills/mcp/vault/local | `src/tui/palette/palette.rs` (for commands) + `src/tui/components/list_row.rs` | Phase 2 |
| `UI::active_modal` + `Modal::QuitConfirm` | `FocusStack` + (later) ratatui-kit `ConfirmModal` | Phase 4 |
| Slash-command parsing in `input_events.rs` Enter handler | `CommandRegistry` (palette) | Phase 2 |
| `SettingsTab::Keybindings` rendering | `src/tui/settings/editors/keybind.rs` | Phase 3 |
| `UI::sidebar_open` + `artifact_sidebar_open` + `SIDEBAR_WIDTH` constants | `PanelSizes` in `src/tui/layout/panels.rs` | Phase 5 |
| `src/app/settings.rs::load_settings_popup_state` | `AppSnapshot::settings_view()` | Phase 3 |
| Per-overlay `centered_rect` helpers | single helper in `src/tui/components/panel.rs` (or ratatui-kit `Center`) | incremental |

---

## 6. Command palette design

### 6.1 Command registry

Each command:

```rust
pub struct Command {
    pub id: &'static str,                         // "chat.new"
    pub title: &'static str,                      // "New chat"
    pub description: &'static str,                // "Start a new conversation"
    pub category: CommandCategory,                // Chat, Provider, Model, Theme, Layout, Mcp, Obsidian, Local, Settings, Help
    pub keywords: &'static [&'static str],        // ["new", "conversation", "start"]
    pub shortcut: Option<Shortcut>,               // Some(Ctrl+N)
    pub enabled: fn(&AppSnapshot) -> bool,        // contextual
    pub action: Action,                           // Action::NewChat
    pub preview: Option<PreviewFn>,               // optional right-pane detail
    pub setting_target: Option<SettingId>,        // opens the settings popup scrolled to a setting
}
```

`CommandRegistry` is built at startup (`OnceLock`) from a `&[Command]` literal
plus dynamic commands discovered at runtime (provider list, model list,
sessions, themes, MCP servers, slash commands).

### 6.2 Concrete commands for this app

| id | title | category | shortcut | action | setting_target |
|---|---|---|---|---|---|
| `chat.new` | New chat | Chat | Ctrl+N | `NewChat` | — |
| `chat.close` | Close current chat | Chat | Ctrl+W | `RemoveTab(active)` | — |
| `chat.clear` | Clear current conversation | Chat | — | `ClearConversation` | — |
| `chat.export` | Export chat | Chat | Ctrl+E | `ExportConversation` | — |
| `provider.switch` | Switch provider | Provider | — | opens list popup | `provider.default` |
| `provider.add` | Add provider | Provider | — | `SetSetting(.., Provider)` | `provider.add` |
| `model.switch` | Switch model | Model | — | opens list popup | `model.default` |
| `model.refresh` | Refresh model list | Model | Ctrl+R | `RefreshModels` | — |
| `session.open` | Open conversation | Session | — | `LoadConversation(id)` | — |
| `session.search` | Search conversations | Session | — | opens search popup | — |
| `theme.switch` | Change theme | Theme | — | opens theme list | `theme.active` |
| `theme.cycle` | Cycle theme | Theme | — | `SetSetting(theme, next)` | `theme.active` |
| `sidebar.toggle` | Toggle sidebar | Layout | Ctrl+B | `ToggleSidebar` | `layout.sidebar_visible` |
| `sidebar.resize.left` | Sidebar narrower | Layout | Alt+H | `ResizeSidebar(-1)` | `layout.sidebar_width` |
| `sidebar.resize.right` | Sidebar wider | Layout | Alt+L | `ResizeSidebar(+1)` | `layout.sidebar_width` |
| `sidebar.reset` | Reset layout | Layout | Alt+0 | `ResetLayout` | `layout.*` |
| `artifact.toggle` | Toggle artifact sidebar | Layout | Ctrl+] | `ToggleArtifactSidebar` | — |
| `settings.open` | Open settings | Settings | Ctrl+, | `OpenSettings` | — |
| `settings.open.theme` | Open settings → Themes | Settings | — | `OpenSettings(Themes)` | `theme.active` |
| `settings.open.keybinds` | Open settings → Keybinds | Settings | — | `OpenSettings(Keybinds)` | — |
| `mcp.open` | Open MCP config | Mcp | — | `OpenSettings(Mcp)` | `mcp.servers` |
| `mcp.connect` | Connect MCP server | Mcp | — | `McpConnect(name)` | `mcp.servers` |
| `obsidian.open` | Open Obsidian vault | Obsidian | — | `OpenVault` | `obsidian.vault_path` |
| `obsidian.search` | Search vault | Obsidian | — | `ShowLocalSearch("")` | — |
| `local.toggle` | Toggle local inference | Local | — | `SetSetting(local.enabled, !)` | `local.enabled` |
| `markdown.toggle` | Toggle markdown rendering | Chat | — | `SetSetting(chat.markdown, !)` | `chat.markdown` |
| `websearch.toggle` | Toggle web search | Chat | — | `ToggleWebSearch` | `chat.websearch` |
| `keybinds.show` | Show keybinds | Help | — | `OpenKeybinds` | — |
| `help.show` | Show help | Help | ? | `ShowHelp` | — |
| `quit` | Quit | Help | Ctrl+Q | `quit_action()` | — |

### 6.3 Search

- Fuzzy match over `title` + `description` + `keywords` + `category`.
- Rank by: (a) exact title prefix > (b) token-start match > (c) subsequence
  match > (d) category title. Use a small fuzzy crate (e.g. `fuzzy-matcher`
  Smith-Waterman, or port OpenCode's `fuzzysort.go` approach). Chosen crate is
  a Phase 0 dependency decision; `nucleo-matcher` is a good Rust default and
  is already used by modern Ratatui ecosystem projects.
- Blank query shows curated picks: the 10 most recent from history + the
  top-level commands (`chat.new`, `settings.open`, `theme.switch`,
  `provider.switch`, `model.switch`, `sidebar.toggle`, `quit`).
- Disabled commands (`enabled == false`) are hidden entirely, not greyed — less
  noise. Their `enabled` reason could be shown in the preview pane.

### 6.4 Keyboard behavior

| Key | Behavior |
|---|---|
| Ctrl+P | Open palette (global; passes through `Ctrl+P` if not bound) |
| Esc / Ctrl+G | Close palette (Action::ClosePalette) |
| typing | Updates search; selection resets to top match |
| Up / Ctrl+K | Move selection up (wraps) |
| Down / Ctrl+J | Move selection down (wraps) |
| Enter | Run selected command's action; close palette (unless `setting_target` set, in which case close palette and open settings scrolled to that setting) |
| Tab | Switch category filter (or, if a setting is selected, switch to settings popup) |
| Backspace at empty query | Close palette |
| Ctrl+[ | Show full shortcut for the selected command in the preview pane |
| `/` prefix | Restrict to a category: `/set` -> Settings, `/chat` -> Chat (optional, Phase 2.2) |

The palette opens its own input layer (`use_input_layer(open, blocks_lower=true)`) so the
chat input and host shortcuts do not interfere while it is open. Global
emergency shortcuts (Ctrl+C quit, Ctrl+Q) stay on `EventScope::Global` and
remain reachable.

---

## 7. Settings redesign

### 7.1 Categories

- Providers
- Models
- Local inference
- Themes
- Keybinds
- MCP
- Obsidian
- UI layout
- Chat behavior
- Privacy / storage
- Experimental

### 7.2 Setting entry schema

```rust
pub struct Setting {
    pub id: SettingId,                    // "theme.active"
    pub title: &'static str,              // "Theme"
    pub description: &'static str,       // "Color palette for the TUI"
    pub category: SettingCategory,
    pub keywords: &'static [&'static str],
    pub value_type: SettingType,
    pub default: DefaultValue,
    pub validation: Option<Validator>,   // e.g. path must exist, port 1..=65535
    pub read: fn(&AppSnapshot) -> Value,
    pub write: SettingWrite,              // see below — produces the action from the value
    pub requires_restart: bool,
    pub advanced: bool,                   // hidden behind "show advanced"
    pub danger: DangerLevel,             // None | Caution | Destructive
}
pub enum SettingWrite {
    /// Generic: host dispatches `SetSetting(id, value)`; persistence path is decided by host per-id.
    Generic,
    /// Specific: a setting that needs a concrete action (e.g. `ToggleWebSearch`).
    /// The closure receives the edited `Value` and returns the action to dispatch.
    Once(fn(Value) -> Action),
}
pub enum SettingType {
    Bool, Enum(&'static [&'static str]), String, Number{min, max, step: f64},
    Path{must_exist: bool, must_be_dir: bool},
    // Keybind + per-command action are v1 non-goals (kept on the Keybindings tab).
    ProviderSelector, ModelSelector,
}
pub enum Value {
    Bool(bool),
    Enum(&'static str),          // must be one of value_type's Enum variants
    Str(String),
    Num(f64),
    Path(PathBuf),
    Provider(ProviderId),        // typed newtype, not a bare String
    Model(ModelId),              // typed newtype, not a bare String
}
pub enum DangerLevel { None, Caution, Destructive }
```

### 7.3 Setting types supported

- **Boolean** — `Switch`-style row; Space toggles; write immediately.
- **Enum** — `Select` popover; Enter opens a list inside the popup.
- **String** — `Input`; Enter commits, Esc reverts draft.
- **Number** — `Input` with validation; min/max enforced; commit on Enter.
- **Path** — `Input` + `…` button to open a file/dir picker (future; v1 just a
  string input with must-exist validation).
- **ProviderSelector / ModelSelector** — composite editors that pull live lists
  from `AppSnapshot`; selecting a provider filters the model list.
- **Keybind capture** — **v1 non-goal.** Stays on the existing `Keybindings`
  tab; not reimplemented in the schema-driven popup this round.

### 7.4 Rendering

- A single popup (`modal_area = centered_rect(72, 84)`).
- Left rail: category list (ratatui-kit `Select`).
- Top: a search `SearchInput` that filters **across all categories**.
- Main: scrollable list of setting rows for the active category (or filtered
  list if search is non-empty). Each row: title (bold), current value (cyan),
  description (gray) below.
- Editing a row in-place: arrow keys move, Enter activates the editor type
  (toggle, popover, inline input).
- Advanced settings hidden until "show advanced" toggled (a setting itself).
- Destructive settings (e.g. "Clear all conversations", "Reset config")
  render with `theme.error` and require a confirm modal.

### 7.5 Search

- Fuzzy over `title` + `description` + `keywords` + `category`.
- Hides categories rail when search non-empty; shows flat ranked list.
- "Reset to default" available per-row (Backspace on the value? or a `D` key
  when row selected — TBD in Phase 3 design).

### 7.6 Editing, validation, save, revert

- Editing happens on a **draft** in island-local `use_state`.
- Enter commits: `sink.send(setting.write)`; host updates `AppConfig`/storage;
  host pushes a new `AppSnapshot` next frame; island re-renders.
- Esc reverts the draft to the last `read` value.
- Invalid input: row border turns `theme.error`, footer hint explains why,
  Enter is rejected. No save on invalid.
- `requires_restart: true` settings show a "restart needed" badge and emit a
  toast on save.
- **Persistence:** existing pattern (write through to `AppConfig` TOML + the
  `settings` SQLite table) is kept. The host's `dispatch` for
  `Action::SetSetting(id, value)` does the actual write. The schema's `write`
  field is the *action*, not the persistence path — host decides persistence
  per setting (mirroring current `save_settings_popup_state`).
- **Revert:** Esc per row; "Reset all to defaults" command in the popup footer
  (destructive, confirm).

### 7.7 Settings reachable from the palette

Every setting exposes its id; palette commands with `setting_target = Some(id)`
open the settings popup scrolled to that setting. This implements the
"command palette as the fast entry point" half of recommendation C.

---

## 8. Resizable sidebars / panels

### 8.1 Layout regions

```
┌─ Top bar (1 row) ──────────────────────────────────┐
├─ Left sidebar ─┬─ Chat area ──────┬─ Right sidebar ─┤
│  conversations │  messages        │  artifacts /     │
│                │  + input         │  inspector       │
├─ Status bar (1 row) ─────────────────────────────────┘
   (modal overlays, palette, settings popup render on top)
```

### 8.2 State model

```rust
pub struct PanelState {
    pub left_visible: bool,
    pub left_width: u16,        // 0..=64
    pub right_visible: bool,
    pub right_width: u16,       // 0..=48
    pub input_min_rows: u16,    // 3..=12
    pub input_height: u16,      // dynamic; grows with content up to max
    pub status_visible: bool,
}
impl PanelState {
    pub const MIN_LEFT: u16 = 16;
    pub const MAX_LEFT: u16 = 48;
    pub const MIN_RIGHT: u16 = 20;
    pub const MAX_RIGHT: u16 = 48;
}
```

Persisted alongside `AppConfig` (small additive change; see §13).

### 8.3 Resizing — keyboard + setting only, **no mouse drag**

Per user direction: sidebar width is a **persisted setting**, adjusted by
keyboard. Mouse drag-resize is **out of scope** (it would require a crossterm
`Down`→`Drag`→`Up` capture state in `handle_mouse`, which today only handles
`Down(Left)`/`Scroll*`, and would fight the existing hit-area pattern). The
width is fully controllable from the settings popup (§7) under *UI layout* and
from the keyboard:

| Key | Action |
|---|---|
| Alt+H | `left_width -= 1` (clamp to MIN_LEFT; if at MIN, hide) |
| Alt+L | `left_width += 1` (clamp to MAX_LEFT) |
| Alt+Shift+H | `left_width -= 5` |
| Alt+Shift+L | `left_width += 5` |
| Alt+] | toggle right sidebar visibility |
| Alt+[ | toggle left sidebar visibility |
| Alt+0 | Reset layout to defaults |
| Alt+Backspace | Toggle status bar |

The right sidebar width is set via the settings popup (no per-press keyboard
loop in v1 — low ROI; add later if requested).

### 8.4 Small-terminal behavior

- Below `width < 100`: hide the right sidebar automatically and refuse to open
  it (show a toast "right sidebar hidden due to terminal width").
- Below `width < 70`: hide the left sidebar automatically.
- These "auto-hide" states override `left_visible`/`right_visible` until the
  terminal grows back; the persisted `left_visible` is restored on resize.
- Input area shrinks toward `input_min_rows=3`.

### 8.5 Persistence

`PanelState` is stored in `AppConfig` (`~/.config/tcui/config.toml`) under a
new `[tui.panel]` section (additive). Loaded at startup, written on change.

### 8.6 Reset / hide commands

A single `Action::ResetLayout` restores defaults and re-shows sidebars (subject
to small-terminal rules). `Action::ToggleSidebar` keeps existing semantics;
new `Action::HideAllPanels` (Alt+Backspace?) for focus mode.

### 8.7 Implementation

Use plain Ratatui `Layout` with `Constraint::Length(panel_state.left_width)`
and `Constraint::Min(0)` for the chat area. The current `render` body in
`src/ui/mod.rs` is refactored to read from `panel_state` instead of the
`sidebar::SIDEBAR_WIDTH` constant and the inline `artifact_width` math.

---

## 9. Better input fields

### 9.1 Options evaluated

- **ratatui-kit `Input`/`SearchInput`** — single-line, input-layer-aware,
  good for palette/search/settings string fields — **only if Phase 0 (Q-E)
  passes**. `textarea` feature is **offline in 0.6** — no built-in multiline.
  Fallback (or always, even if Q-E passes): the `tui-input` crate, which is
  what r-tk's `input` feature is built on. Same widget, no r-tk layer.
- **`tui-textarea`** — mature, supports multi-line, selection, yank, undo.
  **Caveat (corrected from v0):** `tui-textarea` does **not** ship `$EDITOR`
  handoff — that pattern is implemented by us, reusing the existing
  `EditorPopupState` that already handles `Ctrl+E` for artifact editing. Do
  not claim a built-in feature that isn't there.
- **`tui-input`** — single-line; the host can use it directly without r-tk.
- **Existing custom input** (`src/app/input.rs`: `input_content`/`input_cursor`
  char-index pair, mutated by ~15 call sites incl. `browse_input_history`,
  `set_input_cursor_from_click`, slash parsing, send) — works for v1; no
  selection, no paste detection, but it is load-bearing and entangled.

### 9.2 Recommendation: keep current input for v1, migrate later behind a wrapper

**Do not** flip the source of truth to `tui-textarea` mid-refactor and do
**not** `sync on focus/blur` (two sources of truth, desync bugs). Two viable
paths, decided at Phase 6 time:

- **9.2.a (preferred for v1):** keep the current `input_content`/`input_cursor`
  pair as ground truth; render it through a thin `chat_input.rs` host widget
  in Phase 5/6 that *draws* like a textarea but *reads* the existing fields.
  Cheapest, no behavior regression; gets prettier rendering without behavior
  change. Slash commands, history, click-positioning all keep working.
- **9.2.b (later, opt-in):** introduce a `ChatInputState` wrapper that owns a
  `tui_textarea::TextArea` *as* the source of truth and migrate **all ~15 call
  sites together** in one PR (send, history, slash parsing, click placement,
  scroll, render). No focus/blur sync — single owner. Bigger PR; ship only
  after the focus/overlay work (Phase 4) is stable.

Either path keeps the existing `EditorPopupState`-based `Ctrl+E` external
editor working; 9.2.b just reuses it unchanged.

### 9.3 Requirements coverage

| Requirement | Chat input (tui-textarea) | Single-line (ratatui-kit Input/SearchInput) |
|---|---|---|
| Multiline | Yes | n/a |
| Single line | n/a | Yes |
| Cursor movement | Yes (arrows, Home/End, Ctrl+arrows word) | Yes |
| Selection | Yes (Shift+arrows) | limited (ratatui-kit Input supports basic selection) |
| Paste | tui-textarea has bracketed-paste support via crossterm events | via crossterm paste event forwarding |
| Submit | Enter (Ctrl+Enter for literal newline) | Enter |
| Cancel | Esc clears draft / Ctrl+G | Esc closes popup |
| External editor | `Ctrl+E` opens `$EDITOR` with current content (tui-textarea supports this pattern; we wire `EditorPopupState` to it) | n/a |
| Focus integration | host focus manager decides if chat or palette has it | island owns the input layer while active |

### 9.4 Which input uses which component

- Chat input → `tui-textarea` (host) (Phase 6).
- Palette search → ratatui-kit `SearchInput` (island) (Phase 2).
- Settings string/path/number → ratatui-kit `Input` (island) (Phase 3).
- Save-file path, export path → migrate to ratatui-kit `Input` when those
  become islands (Phase 4 — out of v1 scope but listed).

---

## 10. Focus and modal model

### 10.1 Focus states

```rust
pub enum Focus {
    Chat,                  // chat transcript + chat input (host)
    Sidebar,              // left conversation list (host)
    ArtifactSidebar,      // right panel (host)
    Palette,              // overlay (r-tk island if Q-E, plain Ratatui otherwise)
    SettingsPopup,        // overlay (r-tk island if Q-E, plain Ratatui otherwise)
    Modal(ModalId),       // host overlay (r-tk ConfirmModal only if Q-E)
    EditorPopup,          // host
    ListPopup,            // host (kept for transient popups)
}
pub struct FocusStack { stack: Vec<Focus> }  // top = active
```

The stack replaces the boolean flags. `open(Focus)` pushes; `close()` pops.
Reading "what's active" is `stack.last()`.

### 10.2 Event priority (host)

`handle_key` becomes a single match on `focus.top()`:

1. **Global emergency shortcuts** (always): Ctrl+C (quit), Ctrl+Q (quit),
   Ctrl+P (open palette — even from inside another modal, palette wins to
   feel like OpenCode), Ctrl+L (redraw).
2. **Top of stack:** dispatch to:
   - `Palette` → palette overlay's `handle_key`
   - `SettingsPopup` → settings overlay's `handle_key`
   - `Modal(id)` / `EditorPopup` / `ListPopup` → existing per-overlay handler
   - `Chat` / `Sidebar` / `ArtifactSidebar` → existing host handlers.
3. **App-level shortcuts** (when stack top is `Chat` and no field is focused):
   Ctrl+B, Ctrl+], Ctrl+T, Ctrl+N, Ctrl+W, Ctrl+,, Alt+H/L, etc.

This replaces the 600-line cascade with a small match. Overlays no longer need
to check each other's flags.

### 10.3 Overlay event contract — host owns events in both paths

**The host owns all raw crossterm events.** `TuiApp::run` already polls
crossterm in its `tokio::select!` and feeds `handle_key`/`handle_mouse`. That
does not change. Each overlay (palette, settings, modal) exposes a
**host-level** contract:

```rust
trait Overlay {
    fn handle_key(&mut self, key: KeyEvent, host: &mut TuiApp) -> Option<Action>;
    fn handle_mouse(&mut self, mouse: MouseEvent, host: &mut TuiApp) -> Option<Action>;
    fn render(&mut self, f: &mut Frame, area: Rect);
}
```

In the **plain-Ratatui fallback** (and for `Modal`/`EditorPopup`/`ListPopup`
always), the host calls `overlay.handle_key(key, &mut self)` directly — same
shape as today's cascade, just one dispatch arm per `Focus` variant.

In the **r-tk island path (only if Q-E passes)**, the island is *rendered* by
r-tk into a sub-rect and *receives events* via the host's
`overlay.handle_key`. If Q-E proves r-tk's `InputRuntime` can pump events
itself from outside `fullscreen()`, the host forwards; if not,
`overlay.handle_key` is its own input handler that internally drives the r-tk
subtree's state. **Either way the host's `FocusStack` is the canonical routing
point — there is no second event system behind the host's back.**

This resolves the v0 contradiction (which said r-tk's `InputRuntime` would
receive events "because the island is part of the ratatui-kit tree" —
unproven, and only true if the whole app went `fullscreen()`, which we forbid).

### 10.4 Focus transitions

| From | Trigger | To |
|---|---|---|
| Chat | Ctrl+P | Palette |
| Palette | Esc | Chat (restore prior focus) |
| Palette | Enter on command | (action) then Chat |
| Palette | Enter on command targeting setting | SettingsPopup |
| SettingsPopup | Esc | prior focus (Chat or Palette) |
| Chat | Ctrl+, | SettingsPopup |
| Any | Modal opened | Modal pushed |
| Modal | Esc / confirm | pop |

Restoration uses the stack: closing an overlay pops, never forces a specific
target. This naturally handles "open settings from palette → close settings →
back to palette → close palette → back to chat."

### 10.5 Global emergency vs. palette

If `Ctrl+P` should work even inside settings (OpenCode-like), it must be on the
host's global tier, **above** the island's input layer. In ratatui-kit terms,
this is `EventScope::Global` (never cut off by `blocks_lower`). The host
checks `Ctrl+P` first in all branches, opens the palette, and the palette's
input layer takes over.

---

## 11. Migration plan (incremental, PR-sized phases)

Each phase is ~1 PR. Every phase lands behind a runtime flag
(`--tui-v2` or an `AppConfig` boolean) for safe rollout, and **no phase is
allowed to break the existing UI**. The order below is the load-bearing change
from v0: **focus routing lands before any palette/settings UI**, because the
palette/settings both key off the `FocusStack`. Phase 0 is a disposable spike;
Phases 1+ are the real work.

### Phase 0 — Q-E spike + dependency compatibility (PR 0, disposable)
A pure investigation PR. **Do not** add `src/tui/` skeleton or app changes here
(v0's mistake — that pre-loaded the answer). Two outcomes only:

1. **Coexistence check.** Add `ratatui-kit = { version = "0.6", features =
   ["input", "atom"] }` to `Cargo.toml`, `cargo check`. Confirm an edition-2021
   host compiles against an edition-2024 dependency. If a workspace split is
   required, document it.
2. **Q-E answer.** Read the `ratatui-kit` source (clone the v0.6 tag, or use
   `cargo doc --open`) for **any public, non-`fullscreen()` API** that:
   (a) renders an `element!`/`AnyElement` tree into a caller-provided
   `Frame`/`Buffer`/`Rect`, AND/OR (b) pumps crossterm events into
   `InputRuntime` from outside the framework's own loop.
   Document the answer in `docs/ui-refactor-qna.md` (Appendix C of the plan,
   see §12). **If no such API exists, every "use r-tk" row in §2 collapses to
   its plain-Ratatui fallback column** and the rest of the plan still ships —
   the registries and `FocusStack` are framework-agnostic.

Outcome of Phase 0 is a markdown note (`docs/ui-refactor-phase0-findings.md`)
answering Q-E with evidence (cited file paths / doc items from r-tk source).
The throwaway `ratatui-kit` dep may be reverted if Q-E fails.

### Phase 1 — Host focus routing + Overlay trait (PR 1, internal)
Land the `FocusStack` + `Overlay` trait + global key tier **on the host** only
(no new UI). This is the contract every later overlay depends on, so it must
land first (Oracle: "land the host focus/router contract before any ratatui-kit
or palette UI").

- Add `src/tui/focus/manager.rs` with `FocusStack` + `Focus` enum (§10).
- Wire it into `UI` **beside** (not replacing) the existing boolean flags; the
  stack is the new canonical read but the flags are kept as a single source the
  stack mirrors until Phase 4.
- Define the `Overlay` trait (§10.3) and give each existing overlay
  (`active_modal`, `delete_confirm`, `save_file_dialog`, `export_dialog`,
  `editor_popup`, `list_popup`, `show_settings` → `SettingsPopup`) a thin
  `Overlay` impl that just calls its existing handler — **zero behavior
  change**.
- Add the global emergency tier in `handle_key` (Ctrl+C/Q, Ctrl+P stubs to a
  no-op pending Phase 3, Ctrl+L) and reshape `handle_key` into
  `match focus.top() { … }`. The 600-line cascade becomes one dispatch arm per
  `Focus` variant.
- No user-visible behavior change.

### Phase 2 — Registries substrate (PR 2, internal)
- `src/tui/app_bridge.rs`: `AppSnapshot<'a>` (read-only view into `UI` + the
  config read-lock), `ActionSink` (wraps `action_tx`).
- `src/tui/palette/registry.rs` + `command.rs`: build the static command list
  from §6.2. Dynamic commands (themes, providers, models, sessions) added via
  `extend_dynamic(&AppSnapshot)`. Add `nucleo-matcher`.
- `src/tui/settings/schema.rs` + `registry.rs`: declare all settings from the
  current `SettingsPopup` 60 fields as `Setting` entries with `read`/`write`.
- Add the new `Action`s: `OpenPalette`, `ClosePalette`,
  `OpenSettings(SettingCategory)`, `SetSetting(SettingId, Value)`, the
  resize/hide/reset actions. The host's `dispatch` handles them (mostly thin
  wrappers around existing logic).
- Slash-command parsing in `input_events.rs` is **kept** (backward compat —
  do not yet deprecate; Oracle flagged replacing them inside the first
  palette PR as too much).
- No palette/settings UI yet. Both registries are unit-tested.

### Phase 3 — Command palette UI (PR 3, user-visible)
- `src/tui/palette/palette.rs`: render the palette and handle its keys.
  - **If Q-E passed:** r-tk `Modal` + `SearchInput` + `Select`.
  - **If Q-E failed (or as a safer first cut):** plain Ratatui popup driven by
    `CommandRegistry` + `tui-input` for the field + a hand `StatefulList` for
    results. The two paths share `CommandRegistry`, so this PR does NOT block
    on Phase 0's outcome.
- Host opens on `Ctrl+P` (already in the global tier from Phase 1) via
  `FocusStack::open(Palette)`. Selected command emits its `Action` through
  `ActionSink`.
- **Slash commands stay**; the palette is additive. Users who know the
  slashes keep using them; new users find the palette.
- `ListPopup` usages for theme/skills/mcp/local-search are unchanged this PR
  (they migrate gradually in a later cleanup PR — they're transient pickers,
  not part of the palette itself).

### Phase 4a — Settings schema browser (PR 4a, user-visible)
- `src/tui/settings/popup.rs`: a **read-only searchable browser** of
  `SettingRegistry`. Left rail = categories, top = search across all
  categories, main = list of setting rows (title + current value +
  description). No editing yet.
  - Same r-tk-vs-plain branch as Phase 3.
- Host opens via `settings.open` palette command (Phase 3 already wired),
  `Ctrl+,`, or `Action::OpenSettings(category)`.
- The **old `SettingsPopup` still exists** for editing in 4a — users can still
  edit through it. The new popup is a *discovery* surface only.
- This split is Oracle's "settings PR is too big": schema+browser first,
  editing+persistence second.

### Phase 4b — Settings editing + persistence + remove old (PR 4b, user-visible)
- Add per-type editors (bool toggle, enum `Select`, string/number `Input`,
  path, provider/model selectors) to the new popup.
- Wire `Setting::write` → `Action::SetSetting(id, value)` (or
  `SettingWrite::Once` for specific actions like `ToggleWebSearch`).
- Host's `dispatch` for `SetSetting` does the actual persistence (TOML +
  SQLite, mirroring current `save_settings_popup_state`).
- Re-derive `UI` flags from `AppConfig` on each frame (kill the manual 4-way
  mirroring on close — it duplicates `AppConfig` into `UI` then forgets).
- **Delete the old `SettingsPopup`** and its ~250-line key handler branch in
  `input_events.rs`. The `Keybindings` tab *content* moves to a settings
  sub-page in the new popup OR stays as a v1 stub whose editor is Phase 6+
  (keybind capture is a v1 non-goal — see §13).
- Do **not** merge 4b until Phase 4a has shipped stable.

### Phase 5 — Resizable panels + layout refactors (PR 5, user-visible)
- `src/tui/layout/panels.rs`: `PanelState` on `UI`, persisted to
  `AppConfig[tui.panel]` (additive).
- Refactor `UI::render` body to read from `panel_state` instead of the
  `sidebar::SIDEBAR_WIDTH` constant and the inline `artifact_width` math.
- Add the keyboard resize/hide/reset keybinds from §8 and the palette commands
  (`sidebar.toggle`, `sidebar.resize.left/right`, `sidebar.reset`,
  `artifact.toggle`). **No mouse drag-resize** (v1 non-goal §13).
- Small-terminal auto-hide rules from §8.4.

### Phase 6 — Chat input upgrade (PR 6, user-visible, optional timing)
Per §9.2: pick **9.2.a** (thin render-only wrapper, cheapest, no behavior
change) or **9.2.b** (`ChatInputState` owns `tui_textarea::TextArea`,
migrate all ~15 call sites together — bigger PR, single source of truth).
Decision made at Phase 6 time based on Phase 1–5 stability.

- 9.2.a add: pretty-render chat input; `EditorPopupState`-based `Ctrl+E` to
  `$EDITOR` stays as-is.
- 9.2.b add: `tui-textarea` dep, `ChatInputState` migration, selection, paste,
  multiline cursor/scroll. Optional: persist history to a small
  `input_history` table.
- **Do not** merge Phase 6 until Phase 1 (focus) is stable in production, or
  the new input risks fighting the overlay stack.

### Phase 7 (optional/deferred) — Sidebar list VirtualList
- Only if Q-E passed and Phases 3/4a proved ROI. Replace `sidebar.rs`
  conversation rendering with a r-tk `VirtualList` island.
- Out of v1 scope; revisit after Phases 0–6 ship.

### PR flagging & merge rules
- Each phase is one PR. **Hard ordering:** Phase 1 → Phase 2 → Phase 3 →
  (Phase 4a → Phase 4b) → Phase 5 → Phase 6. Phase 0 is independent and
  disposable.
- Phases 3, 4a, 4b, 5, 6 are user-visible and warrant a release note.
  Phases 0, 1, 2 are internal.
- Do **not** merge Phase 3 (palette) until Phase 1 (focus routing) is in.
- Do **not** merge Phase 4b (editing + delete old) until Phase 4a (browser)
  has shipped stable.
- Do **not** merge Phase 6 (chat input) until Phase 1 is stable, or the new
  input risks fighting the overlay stack.
- Each phase ships behind the `--tui-v2` flag; flip default to v2 after a
  phase has been on by default for one release without escalations.

---

## 12. Open questions and Phase 0 spike criteria

Phase 0 must answer these with evidence (cited r-tk source paths or doc
items). Until they're answered, the "use r-tk" rows in §2 are conditional.

### 12.1 Q-E (load-bearing)

**Q-E.1** — Does ratatui-kit 0.6 expose a public, non-`fullscreen()` API to
render an `element!`/`AnyElement` tree into a caller-provided
`ratatui::Frame`/`Buffer`/`Rect`? (e.g. an `App::draw_into(&mut frame, area)`
or an `Element::render(area, &mut buf)` on a `Component`/`Updater`.)
**Q-E.2** — If Q-E.1 is yes, does the rendered subtree still receive crossterm
events through its own `InputRuntime`, or must the host forward events via
`Overlay::handle_key` into something like `app.dispatch_event(event)`?
**Q-E.3** — If Q-E.1 is yes, does reactive state (`State::set`/`use_atom`)
trigger a redraw of *just* the subtree, or does it require the whole app to be
on r-tk's loop?
**Q-E.4** — Does the edition-2021 → edition-2024 dependency work without a
workspace split, or do we need a separate guest crate with `[workspace]`?

**Branch:** if Q-E.1 is yes AND Q-E.2/Q-E.3 confirm events/redraw either via
r-tk or via explicit host forwarding → "use r-tk" path is viable for Phase 3+
palette/settings. Otherwise → **plain-Ratatui fallback**, reusing the same
`CommandRegistry`/`SettingRegistry`/`FocusStack`. The fallback is not a
degradation — it's the same UX delivered through host-owned popups.

### 12.2 Secondary open questions (decided during Phase 1–2)

- **Q-S.1** — Should `AppSnapshot` borrow `&UI` per frame (zero copy, lifetime
  juggling) or clone a small DTO (permissive, predictable)? Affects the
  `Overlay::handle_key` signature.
- **Q-S.2** — `tui-input` vs `nucleo-matcher`'s built-in for the palette
  field: is the search UX good enough with a plain regex prefix match for v1,
  or is fuzzy must-have from day one?
- **Q-S.3** — Keep the existing `ListPopup` for theme/skills/mcp pickers and
  just palette-launch them, or rebuild each as a small `Select` popover?
  Lean: keep `ListPopup`, palette just *opens* them.
- **Q-S.4** — `Keybindings` tab fate: ship a v1 stub in the new settings
  popup (read-only list, edit via the old tab) or hold it back to Phase 7?
  Per §13 v1 non-goal, keybind editing is deferred; pick "stub."

### 12.3 Cross-references to open in the no-ratatui-kit plan

The separate `ui-refactor_no_ratatui-kit-plan.md` (drafted after the user
answers `ui-refactor-qna.md`) **does not depend on Q-E** — it commits to the
plain-Ratatui fallback by default. The QnA document collects the specifics
the no-r-tk plan needs (popup layout, list widget choice, search input
component, keybind discovery shape, settings rendering, etc.).

---

## 13. Non-goals

Explicitly **not** in scope during this refactor:

1. **No backend rewrite.** `src/llm/`, `src/mcp/` (client/registry/transport),
   provider abstraction, streaming, and tool-call handling are unchanged.
2. **No provider rewrite.** The `Provider` enum, endpoint tables, auth, OCR
   token files — untouched.
3. **No storage rewrite.** `Storage`, schema, SQLite tables — untouched. The
   only storage change permitted is small additive tables/columns for settings
   persistence (Phase 4b `SetSetting`) and optionally input history (Phase 6).
4. **No full ratatui-kit rewrite.** The host render loop, `UI::render`, the
   chat transcript, markdown, image, status bar, top bar, tab bar, sidebar,
   artifact sidebar stay plain Ratatui for v1. Only palette + settings + focus
   + panels + chat input can adopt ratatui-kit, and only if Q-E passes.
5. **No host-loop replacement via `fullscreen()`.** We never move the root
   rendering into ratatui-kit or hand the crossterm event stream to r-tk's
   `InputRuntime` as the sole consumer.
6. **No r-tk `InputRuntime` ownership unless Phase 0 proves it.** The host
   owns all raw crossterm events; r-tk's event system, if used at all, is fed
   by the host via the `Overlay::handle_key` contract (§10.3).
7. **No app identity/branding redesign.** `DESIGN.md` palette, typography
   tokens, the cyan-accent chat-command-strip identity all stay.
8. **No routing adoption.** ratatui-kit `RouterProvider` is not used.
9. **No re-implementation of `MarkdownRenderer`** or kitty/image/diff rendering.
10. **No change to `Action`'s role as the central bus.** We add actions
    (`OpenPalette`, `ClosePalette`, `OpenSettings(Category)`, `SetSetting`,
    resize actions) but do not replace it.
11. **No removal of `action_tx`/`action_rx`** in favor of reactive atoms.
    Background async work keeps using the channel.
12. **No `which-key` overlay in v1.** Listed as deferred (post-Phase 6).
13. **No mouse drag-resize, left or right.** Per user direction: sidebar width
    is a persisted setting adjusted by keyboard (§8). Right-sidebar drag was
    already out; left is now also out for v1.
14. **No v1 keybind capture/editing.** Stays on the existing `Keybindings`
    tab content; a schema-driven keybind editor is post-v1 (§7, §9).
15. **No theme system replacement.** `crate::theme` stays; overlays consume it.
16. **No new chat transcription features** (charts, mermaid, tables) under
    this refactor — those are `PLAN.md`'s graphics track, not this UX track.
17. **No change to the `v0.6.0` snapshot history** — work happens on
    `development` and merges via PR into `main`.
18. **No deprecation of existing slash commands in v1.** Palette is additive;
    `/theme`, `/skills`, `/mcp`, `/vault` keep working. Revisit post-v1.

---

## Appendix A — Key files reviewed

- `src/main.rs` — entry, terminal setup.
- `src/app.rs` — `TuiApp`, `UI`, `ChatTabState` structs; `new`.
- `src/app/runtime.rs` — `run`, `dispatch`.
- `src/app/action.rs` — `Action` enum (60 variants).
- `src/app/input_events.rs` — `handle_key`, `handle_mouse`,
  `handle_mouse_click` (the 600+ / 1748-line dispatch file).
- `src/app/input.rs` — chat input primitives.
- `src/app/settings.rs` — `load_settings_popup_state`,
  `save_settings_popup_state`.
- `src/ui/mod.rs` — `UI::render`, the god struct, `Modal` enum.
- `src/ui/chat_tab.rs` — `ChatTab`, `RenderedMessages`.
- `src/ui/settings_tab/mod.rs` — `SettingsPopup`, `SettingsTab`.
- `src/ui/settings_tab/state.rs` — `ProviderFormState`, etc.
- `src/ui/sidebar.rs`, `src/ui/top_bar.rs`, `src/ui/status_bar.rs`,
  `src/ui/tab_bar.rs` — kept host widgets.
- `src/ui/artifact_sidebar.rs` — right panel/catalogs.
- `src/ui/modals/quit_confirm.rs`, `src/ui/modals/list_popup.rs`,
  `src/ui/modals/artifact_viewer.rs`, `src/ui/modals/export_dialog.rs`,
  `src/ui/modals/save_file.rs`, `src/ui/modals/editor_popup.rs` — overlays.
- `src/ui/components/markdown_model.rs`, `src/ui/components/image_block.rs`,
  `src/ui/components/terminal_capabilities.rs`, `src/ui/components/chat_message.rs`,
  `src/ui/components/collapsible.rs` — kept components.
- `Cargo.toml` — `ratatui = 0.30`, `crossterm 0.29`, `tokio 1`, edition 2021.
- `ARCHITECTURE.md`, `DESIGN.md`, `PLAN.md` — repo docs.

## Appendix B — OpenCode behavior sources (from librarian research)

- **Palette** `command.palette.show` default `ctrl+p` (TUI); fuzzy over
  display/category/description/keywords; `DialogSelectFile` merges
  commands+files+sessions; Esc closes, Up/Down or Ctrl+P/N navigates.
- **Settings** is a tabbed modal (General/Shortcuts/Servers/Providers/Models);
  search is tab-local, not global. (We diverge by adding global search.)
- **Keybinds** have two discovery surfaces: Settings → Shortcuts (fuzzy list,
  click-to-capture, Backspace clears, conflicts rejected) and a `which-key`
  overlay with persisted layout + pending preview.
- **Theme/model/provider** are available via palette commands and dedicated
  popovers.
- **Layout** persists `sidebar.width`, `terminal.height`, etc. No drag-resize
  in the inspected CLI panels.
- **TUI config** in `tui.json` (theme, keybinds, leader_timeout, mouse, …).
- **Modal/focus** uses a `DialogProvider` stack (show clears, push stacks;
  Escape closes top; close has a settle timeout) and mode-based keymap layers
  with priorities (which-key = 1000, footer subagent = 1). The prompt disables
  base bindings while a menu is visible.
- **Input:** chat is a `contenteditable` div in the web app; palette/settings
  use `TextField`/`SelectV2`; external editor handoff via `openEditor()` with
  a temp file.