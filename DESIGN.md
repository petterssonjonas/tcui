# TermChatUI Design System

## 1. Atmosphere & Identity

A focused terminal chat workspace: calm, dense, and readable under long sessions. The signature is a command-strip chat surface where provider, model, tools, and status stay visible without turning the terminal into a dashboard.

## 2. Color

### Palette

| Role | Token | Light | Dark | Usage |
|------|-------|-------|------|-------|
| Surface/primary | theme-background | terminal-default | dark theme neutral | Chat viewport where the conversation happens |
| Surface/elevated | theme-panel | terminal-black | raised dark theme neutral | Popups, dropdowns, status bar |
| Surface/sidebar | theme-sidebar | terminal-black | one step darker than the chat viewport | Side panels and assistant answers |
| Surface/message | theme-card | terminal-black | dark message neutral | User message blocks |
| Surface/selection | theme-selection | terminal-reversed | dark neutral or intentional accent pairing | Selected rows and code labels |
| Text/primary | theme-foreground | terminal-white | light theme foreground | Main messages and selected text |
| Text/secondary | terminal-gray | terminal-gray | terminal-gray | Hints and labels |
| Text/muted | terminal-dark-gray | terminal-dark-gray | terminal-dark-gray | Placeholders |
| Accent/primary | terminal-cyan | terminal-cyan | terminal-cyan | Provider/model controls, focus |
| Accent/secondary | terminal-magenta | terminal-magenta | terminal-magenta | MCP/tool controls |
| Status/success | terminal-green | terminal-green | terminal-green | User messages, connected |
| Status/warning | terminal-yellow | terminal-yellow | terminal-yellow | Cautions, unconfigured state |
| Status/error | terminal-red | terminal-red | terminal-red | Failed calls |

### Rules

- Use terminal colors through `ratatui::style::Color`; no raw ANSI escape sequences in widgets.
- Custom themes keep the canvas and large surfaces dark, with light foreground text. Accent colors must not fill large selection or message regions.
- Custom-theme selections use a dark neutral background with readable foreground text; intentional theme-specific accent pairings are allowed.
- User messages use a dark card-family neutral. Assistant answers use a darker neutral so both remain distinct from the chat viewport.
- Cyan is reserved for chat controls and assistant identity. Green is reserved for user identity and success.
- Warning yellow means the user can fix the state from settings.

## 3. Typography

### Scale

Terminal UI uses the active terminal font. Hierarchy comes from labels, spacing, and modifiers.

| Level | Size | Weight | Line Height | Tracking | Usage |
|-------|------|--------|-------------|----------|-------|
| Title | terminal-cell | bold | 1 cell | 0 | Chat title and active modal headings |
| Body | terminal-cell | normal | 1 cell | 0 | Messages and controls |
| Label | terminal-cell | bold | 1 cell | 0 | Role labels, selected controls |
| Caption | terminal-cell | normal | 1 cell | 0 | Status and hints |

### Font Stack

- Primary: user's terminal font
- Mono: user's terminal font

### Rules

- Do not rely on emoji width. Use ASCII or ratatui symbols with predictable terminal width.
- Keep command labels short enough for 80-column terminals.

## 4. Spacing & Layout

### Base Unit

All spacing is terminal cells.

| Token | Value | Usage |
|-------|-------|-------|
| cell-1 | 1 cell | Inline gaps, separators |
| cell-2 | 2 cells | Dropdown padding and input borders |
| row-1 | 1 row | Status/options rows |
| row-3 | 3 rows | Compact input |
| row-5 | 5 rows | Empty-chat centered input |

### Grid

- Main layout: top bar, chat surface, status bar.
- The top bar is exactly one row and contains the `TCUI` brand followed by app-view tabs.
- Chat controls sit in a single command strip when enabled.
- Sidebar width is 24 cells.

### Rules

- Controls must remain useful at 80 columns.
- The active chat surface is framed as the current workspace pane.
- Avoid nested bordered panels; borders are for the active pane, input, dropdowns, and modals.

## 5. Components

### Chat Command Strip

- **Structure**: provider, model, skills, MCP as four compact status cells.
- **Variants**: configured, missing model, missing key.
- **Spacing**: `cell-1`, `row-1`.
- **States**: selected values use cyan; missing values use yellow.
- **Accessibility**: click areas match visible labels.
- **Motion**: none.

### Chat Pane Frame

- **Structure**: one outer border around the active chat surface.
- **Variants**: empty chat, active chat, streaming.
- **Spacing**: `cell-1`.
- **States**: active border cyan, inactive chrome dark gray.
- **Accessibility**: provider and model remain available in the bottom status bar; the chat viewport has no duplicate title row.
- **Motion**: streaming spinner in status, not inside message text.

### App Tab Strip

- **Structure**: `TCUI`, a permanent `Chat` tab, zero or more numbered placeholder tabs, then `+` immediately after the rightmost tab.
- **Surface**: tabs use the lighter `theme-panel` surface with normal foreground text; the active tab uses the selection surface.
- **States**: `Chat` cannot be closed. Placeholder tabs are closable and preserve the chat workspace while inactive.
- **Content**: placeholder tabs hide both sidebars and the chat status bar, then center a two-column launcher grid in the remaining viewport.
- **Accessibility**: the full visible tab label is clickable; close and add controls have distinct hit areas.

### Conversation Sidebar

- **Structure**: a three-row `[New Chat]` card followed directly by the scrollable pinned/recent conversation list. No sidebar title or chat count appears above it.
- **Surface**: sidebar cards use `theme-sidebar`; `[New Chat]` and keyboard selection use the elevated or selected surface.
- **States**: mouse wheel scrolls the list viewport. When the sidebar owns focus, Up/Down moves a stable selection and Enter activates it; Esc returns focus to chat input.
- **Accessibility**: selection remains visible while navigating, and conversation action hit areas move with the scrolled card.

### Message Input

- **Structure**: bordered paragraph with concise placeholder.
- **Variants**: centered empty-chat input, compact active-chat input.
- **Spacing**: `row-3` or `row-5`.
- **States**: placeholder muted, entered text primary.
- **Accessibility**: visible title explains action.
- **Motion**: none.

### User Message

- **Structure**: a padded bubble capped at 75% of the chat viewport width.
- **Alignment**: left, centered with equal outer margins, or right according to the user alignment setting.
- **Label**: the user name follows the bubble; right-aligned only for right bubbles, otherwise left-aligned.
- **Surface**: `theme-user-bubble`; outer margin remains the chat background.

### Assistant Answer

- **Structure**: an `Assistant` label followed by clean answer text without a visible bubble edge.
- **Surface**: `theme-assistant-bubble`, which must equal `theme-background`.
- **Label color**: Gruvbox orange for Gruvbox themes; `theme-accent-alt` for every other theme.

### Thinking Block

- **Structure**: one inset, padded block containing both the disclosure label and reasoning text.
- **Surface**: terminal black across the complete block; outer inset remains the chat background.
- **Alignment**: reasoning text follows assistant alignment inside the block.

## 6. Motion & Interaction

### Timing

Terminal interactions update per frame; avoid animation unless it clarifies loading.

| Type | Duration | Easing | Usage |
|------|----------|--------|-------|
| Frame | 33ms | linear | Redraw loop |
| Stream | model-driven | linear | Assistant response chunks |
| Spinner | 132ms | linear | Provider call in progress |

### Rules

- Prefer streaming text over spinners.
- Focus and selection must be visible without color alone where practical.

## 7. Depth & Surface

### Strategy

Borders-only. Use single-line borders for inputs, dropdowns, settings, and confirmations. Use tonal terminal colors for status, not shadows.
