# TCUI

TCUI is a terminal helper workspace for Linux commands, troubleshooting, and scripts, backed by local or hosted LLM providers. Chat, live settings, Markdown vaults, and persistent terminal tabs share one terminal-native interface.

TCUI is functional but still a work in progress and not yet ready for a general release. To try it, clone the repo and run `cargo run`. Feedback is welcome.

## Run

```bash
cargo run
```

## Settings

`Ctrl+P` opens **Settings search**. With no query it offers **Settings tab**;
typing searches settings and available actions. Selecting a setting opens its
category on the dedicated Settings tab. `/settings` and the launcher open that
tab directly.

Settings are grouped into Appearance, Terminal, Layout, Chat, Web search,
Connections, Models, Local inference, Notifications, Files & vaults, Keyboard,
MCP, Privacy, and Memory when that feature is enabled. Controls include previously
hidden sidebar widths, status widgets, local probe timeouts, ntfy delivery, memory
limits, and terminal defaults. Provider secrets and vault profiles stay in their
dedicated managers. Inactive legacy fields are not presented as working settings.

Use Tab to switch category/list focus and arrows to navigate. Toggle booleans or
cycle choices directly; Enter edits text and numbers and commits valid values.
Changes persist individually without a global Save button. Errors preserve the
draft and previous saved value. Keyboard actions show current/default bindings:
Enter captures a binding, Enter confirms it, and Backspace resets it. Advanced
MCP and bulk keybinding editors accept validated JSON.

Terminal placement updates the current split immediately. Shell/directory and
provider/model defaults apply to new sessions or conversations; existing sessions
keep their state. Recovery retention changes policy for the next startup recovery
pass, not an immediate purge.

## Commands and terminal tabs

Closed `sh`, `bash`, `shell`, `zsh`, and `fish` code fences in completed assistant
responses expose **Run** next to the top copy control. Run first opens a review;
it never executes merely because a response arrived or the review was opened.

- Up/Down chooses a block; Space or its checkbox selects it. Only the first block
  starts selected. PageUp/PageDown scroll the payload.
- **Copy** or Ctrl+C copies selected commands, not surrounding prose.
- **Run** or Ctrl+R explicitly approves the displayed commands in a **new terminal**.
  The fence interpreter is respected; incompatible mixed interpreters are rejected.
  Clean startup skips shell startup files and waits for shell readiness. Approved
  text is sourced from an owner-only temporary script (`0600` on Unix), not typed
  line by line. Interactive `sudo`, confirmation, and `read` prompts therefore
  receive your input rather than later commands. Sourcing preserves shell variables
  and directory changes. The terminal shows the script invocation; review shows
  its exact contents. Normal terminal close removes the file; forced termination
  can leave a private temporary file behind. Failures never automatically retry commands.
- Tab or **Dest** selects an existing terminal instead. Existing terminals offer
  **Paste**, not Run: Ctrl+V or **Paste** transfers input without adding Enter.
  Inspect the destination and press Enter there yourself. A foreground program
  may receive the paste; shell syntax is not converted. Unsafe unbracketed
  multiline paste is rejected.
- Escape or **Cancel** sends nothing. The review identifies the execution host
  and directory. Over SSH, commands run on the server hosting TCUI, not your laptop.

`/terminal` or the launcher opens an ordinary interactive terminal, even without
a connected provider. Each terminal
remains a tab while shown below chat, to its right, or as a full tab. Header
controls **Tab**, **Right**, and **Bottom**, or F7, change presentation without
restarting the shell. Drag the split divider to resize it. F6 switches chat/terminal
focus; from a full terminal it returns to a bottom split with chat focused.
Clicking either pane also changes focus.

Ctrl+C and Escape reach the focused terminal. Ctrl+P remains settings search.
Shift+PageUp/PageDown scroll terminal history; mouse input reaches terminal
applications that request it. Closing a running terminal, or quitting with live
terminals, requires confirmation. Sessions survive view/tab changes, not application
exit. Detached jobs that leave the PTY session are outside its cleanup boundary.

## Copy text

- Drag with the left mouse button to select visible text. Release to copy it.
  Selection follows terminal cells, including displayed line wrapping; scrolling,
  resizing, or pressing a key cancels an active selection. Graphics-protocol cells
  and hidden image cells are excluded rather than copied as terminal commands.
- Click the small copy icon at the top right of an assistant response to copy
  the complete answer, preserving its original Markdown and newlines. There is
  no duplicate bottom copy control. During streaming, copying uses the answer
  received so far.
- The default copy icon uses Font Awesome (`U+F0C5`, also available in Nerd Fonts).
  If your terminal font lacks it, launch with `TCUI_COPY_ICON=unicode cargo run`
  for `⧉`, or `TCUI_COPY_ICON=ascii cargo run` for the portable `[c]` fallback.
  Terminals do not expose font coverage, so fallback selection is explicit.

Local copying uses `wl-copy` on Wayland, `xclip` or `xsel` on X11, and `pbcopy` on
macOS when available. Remote sessions, or systems without these helpers, use
OSC 52; your terminal must allow clipboard writes. Tmux also needs
`set -g allow-passthrough on`. A “Copy sent to clipboard” notice confirms that the
request was sent, not that an OSC 52 terminal accepted it.

## Providers

Use `/providers` or `/connect` to add a connection. Provider and model selectors
show your saved connections, not the preset catalog. Presets include OpenAI,
Anthropic, Anthropic Agent SDK, Google AI, OpenRouter API key, OpenRouter OAuth,
OpenCode Go, OpenCode Zen, Groq, Mistral, Kilo Gateway, Berget.ai, and OpenAI OAuth.
API-key presets need only a key. **Custom** accepts a name, an OpenAI-compatible
endpoint, and a key. Use `/model <id>` when a custom service cannot list models.

OAuth works locally and over SSH without launching a browser on the app host:
copy the sign-in URL with `c`, authorize in your browser, then paste the complete
final response URL into TCUI and press Enter—even if the browser cannot load
the localhost callback. Press `p` to enter the response field manually.
OpenAI OAuth discovers its current subscription models; `Ctrl+R` refreshes them.

TCUI never automatically reads provider keys from environment variables, `.env`,
or another application's login files. Local inference remains separately
configurable in Settings.

## Web search

**Settings → Web search** selects the source, result limit, optional SearXNG
endpoint, and automatic-search behavior. The default local backend queries
DuckDuckGo Lite directly: no `ddgr`, browser, API key, or MCP subprocess is needed.
“Local” means the request runs on the TCUI host, including over SSH; it is not
offline search.

- `@websearch <query>` searches with the selected source.
- `@research <query>` uses that source for a sourced report.
- `@exa <query>`, `@tavily <query>`, and `@firecrawl <query>` select the named MCP
  service explicitly.

These mentions work while **Automatic web search** is off. `/web` toggles
automatic search before each chat message; turning it on sends each request to
the selected service and can consume that service's credits. Do not include
secrets in search requests. Queries are limited to 2,000 characters, with 1–10
requested results. Tavily currently requires at least five results.
Same-source mentions share one search, and explicit searches do not trigger an
additional automatic search. Failures never silently switch to another source.

For MCP search, open **Search API keys** in that category or find it with
`Ctrl+P`. Select Exa, Tavily, or Firecrawl, choose encrypted-vault or plaintext
database storage, and save the key. Saving enables that service's MCP profile;
it does not change the selected automatic source. Profiles start disabled.
Use `e` to enable/disable, `d` to delete a key, `v` to open the credential vault,
or `h` to explicitly restore the official hosted HTTP preset. This also replaces
an older saved stdio preset without silently overriding custom configuration.
Environment API keys are never imported.

Hosted integrations follow the official [Exa](https://exa.ai/docs/get-started/exa-mcp),
[Tavily](https://docs.tavily.com/documentation/mcp), and
[Firecrawl](https://docs.firecrawl.dev/mcp-server/tools) MCP protocols. They discover
and call only the supported read-only search tool, not crawl, scrape, or
model-selected execution tools. Account access and credits are required.
Advanced MCP settings honor explicit `stdio` commands/arguments or
`streamable_http` URLs. HTTP search keys are restricted to the service's official
HTTPS origin, with redirects disabled. A configured stdio server receives its
explicitly stored service key; configure only commands you trust.

For self-hosted search, set **Local SearXNG endpoint** to an absolute search URL
such as `http://localhost:8080/search` and enable JSON output in SearXNG's
`search.formats`. TCUI supplies `q` and `format=json`; do not put credentials or
query parameters in the endpoint setting. Clearing it restores DuckDuckGo Lite.
See the [SearXNG search API](https://docs.searxng.org/dev/search_api.html).

Retrieved text is bounded, marked as untrusted evidence, and accompanied by
source URLs. Search does not approve or run commands. Missing keys, locked
vaults, disabled services, and upstream failures are reported rather than
presented as successful searches.

## Authentication

Choose password-encrypted storage or explicit plaintext SQLite storage when
connecting. `/vault` manages the **tcui-credentials** vault, including machine
passwords and multiline SSH private keys. An initialized vault prompts for its
password at launch; cancelling keeps encrypted credentials locked without
closing the app. Reopen it with `/vault`.

Storage choice applies to **all provider and search credentials**, including OAuth tokens.
Machine passwords and SSH keys always stay encrypted. Plaintext credentials are
readable by anyone who can read the app database; encryption cannot protect
copies or backups made before conversion. Keep your vault password safe: there
is no password recovery.

OpenRouter uses documented PKCE authorization. OpenAI OAuth uses TCUI's native
Codex flow, which is experimental and non-endorsed. Anthropic Agent SDK uses an
explicit Console API key and requires the official Claude CLI with `--bare`
support; usage draws from your Anthropic API/Console balance, not a Claude
subscription. See [Authentication](docs/auth.md) for setup, CLI usage, migration,
and security details.

## Rendered Markdown

Chat responses and Markdown viewers render tables with Unicode borders,
column alignment, styled cells, clickable links, and width-aware wrapping.
Escaped pipes and pipes inside inline code remain cell content. Very narrow
layouts split wide tables into boxed column panels rather than discard cells.

Recognizable single-column text boxes keep their outer borders aligned even
when generated source has uneven padding. Narrow viewports reflow content inside
the frame. Other code blocks preserve indentation and whitespace, expand tabs
at eight-column stops, and wrap long lines with continuation markers rather
than truncate them. The response copy button still copies the original source.

User messages keep the username on the first row, with a blank row before the
prompt and extra horizontal text padding. Assistant copy controls are inset from
the content edge; the scrollbar sits beyond a separate blank gutter.

Fenced `mermaid` blocks render `flowchart` and `graph` diagrams as connected boxes
and arrows. Compact layouts support labeled connections, branches, shapes, and
subgraphs. When compact routing exhausts its work budget or cannot fit, a bounded
orthogonal layout draws continuous connections, inline edge labels, and group
frames instead. Wide terminals place compatible nodes in multiple columns:
TD/BT branches share rows, while LR/RL graphs use horizontal ranks when they fit.
Narrow or grouped recovery layouts retain stacked boxes. Cycles, return paths,
and parallel edges do not become numbered connection lists.

Mermaid `xychart-beta` (or `xychart`) renders vertical bar charts, XY line charts,
and mixed bar/line series with axes, scales, colors, and legends. X axes accept
categories or numeric ranges; numeric samples are evenly spaced across the range.
Signed values and zero baselines are supported. Dense category keys keep each
entry together when it fits. Mermaid `pie` renders filled, colored sectors with
values and percentages. Pie values must be nonnegative with a positive total.

All rendering is native Rust and works locally or over SSH without a browser or
image-capable terminal. Markdown Off and the copy button preserve original source.
The model instructions in `assets/TCUI.md` include examples for all four visual
formats. The binary embeds these instructions for launches outside the repository;
a working-directory `assets/TCUI.md` remains an optional override.

Unsupported syntax (including horizontal XY charts and interactive directives),
incomplete streamed fences, and inputs beyond viewport/resource limits retain an
explicit, wrapped source view. Limits include 8 KiB of source and 128 source lines;
flowcharts allow 64 nodes, 128 edges, and 16 subgraphs. Charts allow eight series,
256 total plotted values, 64 categories, and 16 pie slices. Repeated redraws reuse
bounded caches.

## Notifications locally and over SSH

SSH does not automatically forward desktop D-Bus. TCUI selects a supported
notification route instead of blindly running `notify-send`:

- **Herdr:** use its native notification integration when the documented pane,
  executable, and socket context is available.
- **Local desktop:** probe the current session's notification service without
  activating it, then use `notify-send`. This requires `busctl` and `notify-send`.
- **SSH terminal:** send notifications through the terminal connection to
  Kitty, Ghostty, or WezTerm. Terminal identification and notification permissions
  must be available on the remote session. A generic `xterm-256color` value alone
  does not identify a notification-capable terminal.
- **No supported route:** desktop notifications are effectively off. SSH sessions
  never fall back to the server's D-Bus or sound service. Unverified multiplexer
  passthrough is disabled.

Response notifications still occur only while TCUI is unfocused. Helper errors
are captured rather than printed over the TUI, and a failed backend is disabled
for the process lifetime. Set `desktop = false` under `[notifications]` in
`config.toml` to disable automatic desktop delivery explicitly. Configured ntfy delivery is
independent.

Detached SSH reminders cannot notify a terminal that is no longer attached.
They preserve Herdr routing context when available; otherwise use explicitly
configured ntfy for delivery after disconnecting.

## Markdown Vault

Markdown Vault connects TCUI to ordinary folders of Markdown files. Add one or
more folders in **Settings → Markdown Vault → Manage vault profiles**, give each
profile a display name, and choose a default. Open a workspace from the `+` tab
launcher, then use its Tree, Viewer, and terminal Editor, or address a profile
from Chat with `@vault`:

```text
@vault list my notes
@vault "Work Notes" read projects/status.md
@vault vault:Personal search for "renewal date"
```

Workspace tabs are ephemeral and are not restored after TCUI restarts; profile
configuration persists. Vault mutations require an exact preview and explicit
confirmation. Generated files are stored once in **Saved files**; **Chat files**
shows conversation-scoped links to those canonical files with View and Del
actions, but no Save action. Deleting a file from either section removes that
physical file and all of its links, while deleting a conversation removes only
its links and preserves the Saved files. For `@save`, TCUI saves the model's
complete file body directly and replaces the visible response with
`Saved as <filename>`.

**Insert to chat** keeps removable document chips in the composer until send.
Sending expands their snapshots in insertion order into the ordinary user
message text. That exact plain text is persisted, displayed, processed, and
included in later conversation context just as if it had been pasted manually;
chip labels and source metadata are not persisted.

See the [canonical Markdown Vault guide](skills/vault/SKILL.md) for profile and
workspace navigation, exact selector grammar, move rollback and recovery,
persisted document expansion, lock behavior, and every enforced limit.

Markdown Vault works with ordinary Markdown folders, including folders used by
Obsidian. TCUI is not affiliated with or endorsed by Obsidian, and Markdown
Vault does not implement Obsidian Sync, APIs, or plugins.

## Install

Release assets for every published release:

- `tcui_<version>_amd64.deb` — Debian/Ubuntu package for x86_64
- `tcui_<version>_arm64.deb` — Debian/Ubuntu package for aarch64/arm64
- `tcui-<version>-1.x86_64.rpm` — RPM package for x86_64
- `tcui-x86_64-unknown-linux-gnu.tar.gz` — portable x86_64 binary
- `tcui-aarch64-unknown-linux-gnu.tar.gz` — portable aarch64 binary
- `tcui-<version>-source.tar.gz` — source archive
- `PKGBUILD` — Arch Linux / AUR `tcui-bin` build script (regenerated per release with the correct `pkgver` and binary checksums)
- `install.sh`, `SHA256SUMS`, `LICENSE`, `potion-base-8M-LICENSE`

Linux install script (detects `uname -m` and the native package manager, downloads the matching `.deb` / `.rpm` / tarball, verifies `SHA256SUMS`, then installs):

```bash
curl -fsSL https://github.com/petterssonjonas/tcui/releases/latest/download/install.sh | bash
```

Options, all optional:

| Env var          | Default                  | Notes |
|------------------|--------------------------|-------|
| `TCUI_VERSION`   | `latest`                 | Release tag, with or without leading `v`. |
| `TCUI_PKG`       | auto                     | Force `deb`, `rpm`, or `tarball` (skips auto-detection). |
| `TCUI_BIN_DIR`   | `~/.local/bin` (or `/usr/local/bin` as root) | Install dir for the tarball fallback only. |
| `TCUI_REPO`      | `petterssonjonas/tcui`   | Override for forks. |

The release workflow (`.github/workflows/release.yml`) builds all of the above on every published release and on manual dispatch.

### Arch Linux / AUR

Each release ships a `PKGBUILD` (`tcui-bin`, sources the matching prebuilt binary tarball for `x86_64` or `aarch64`). Download it from the release and build locally:

```bash
curl -fsSL https://github.com/petterssonjonas/tcui/releases/latest/download/PKGBUILD -o PKGBUILD
makepkg -si
```

The in-repo `packaging/PKGBUILD` is the source of truth the release pipeline regenerates from; AUR maintainers can use it directly when publishing the `tcui-bin` AUR package.

Upgrade in place without touching your config:

```bash
tcui --upgrade
```

## License

TCUI is licensed under GPL-3.0-only. Bundled `potion-base-8M` model assets remain under their
upstream MIT license; the release artifacts ship that license alongside the binary packages.
