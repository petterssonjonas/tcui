# TCUI

TCUI is a terminal helper workspace for Linux commands, troubleshooting, and scripts, backed by local or hosted LLM providers. Chat, live settings, the internal Vault, Memories, and persistent terminal tabs share one terminal-native interface.

TCUI is functional but still a work in progress and not yet ready for a general release. To try it, clone the repo and run `cargo run`. Feedback is welcome.

## Run

```bash
cargo run
```

## App protection

On first launch, choose protection independently for an app-entry password,
credentials, chats, Memories, and the built-in Vault. When a shared password or
protection is already configured, the startup screen lets you unlock it before
opening the app. Unlock stays on the selection screen. Select additional data
types and save without entering the existing password again, then continue to
the app.

Reopen this screen through `/security` or **Settings → Privacy → Application
protection**. Protected chats or an app-entry password require authentication
before config and stores open. With only optional sections protected, you can
continue with those sections locked. Cancelling an unlock returns to protection
options; cancelling the startup screen exits. Unchecked stores remain plaintext.

Choose **Change shared password** after unlocking to set and confirm a new
password. This preserves protection selections and explicit plaintext
connections. `/lock` stops tracked work, persists state, and clears the app
session. Running terminals, unsaved editors, and pending drafts require discard
confirmation for a voluntary transition; cancelling preserves them. Forced
revocation fails closed.

Protection uses a random master secret wrapped by the password-based credential
store, with domain-separated authenticated encryption. Password changes update
encrypted credential rows and managed portable file wrappers, including retained
backups. Interrupted changes require recovery before app data can reopen. There
is no forgotten-password recovery. The data master is not rotated: previously
exported copies keep their old password, and an extracted data key cannot be
revoked by changing the password. Configuration, profile names, paths, filenames,
and logs are not encrypted by these switches. External originals and user-kept
archives or backups are not silently rewritten or deleted.
Copies into chats, exports, or provider requests follow the destination's privacy
rules, not the source's. Locking cannot recall provider-sent data or protect
against the OS account, root, swap, snapshots, or forensic recovery of deleted bytes.

## Conversations and response control

Each response has a request ID, an origin conversation ID, and a destination
conversation ID. Origin and destination normally match. Switching chats does not
move or cancel a response: it continues in its destination, including when that
chat is not visible. Each destination permits one active request; other chats
can run requests independently.

- `/cancel`, or **Escape twice within 1000 ms**, stops the focused chat's running
  response and keeps its partial text. Persistent chats save that text to disk;
  temporary chats keep it in memory. The key sequence applies only to chat input,
  not a dialog or focused terminal. Another key or a view change resets it.
  Old queued events cannot change a later request.
- Deleting a running persistent destination creates a **Recovered** conversation with its
  known history and partial response. The request keeps its original request
  and origin IDs, but uses the new destination ID. Recovery does not overwrite
  unreadable or corrupt chat files.
- `/fork` creates and selects an independent persistent conversation named
  **`Fork of <title>`** in the sidebar. It includes a partial response already received,
  but the original request continues only in its original destination. The fork
  retains the provider, model, and reasoning selection. Send or clear pending
  draft text and inserted documents first. Message history is copied; Chat Artifact
  links are not. Existing Vault items remain available through **Insert to chat**.
- `/steer <instruction>` is reserved for real live steering. Current HTTP SSE
  connections and the Claude print-mode connection cannot accept new input
  during a response. The command reports this, preserves the draft, and leaves
  the request unchanged; it does not simulate steering with cancel-and-restart.

A normal app exit cancels and saves remaining responses in persistent chats.
Cancellation cannot undo completed side effects or guarantee that a remote
provider stops its own processing. Request preparation currently finishes before
the event loop can handle cancellation; the key sequence stops running responses.
Future model-task scheduling can use the same registry with distinct origin and
destination IDs. This does not add a model-task scheduler.

### Temporary forks

`/temp` opens a memory-only copy in the **Wide right sidebar**, titled
**`Ephemeral fork of <title> conversation`**. It copies the current messages,
including a live partial response, but does not move the original request.
The main chat and temporary chat have separate drafts, inserted documents,
and requests. Click a pane, or use **Alt+Left** for main and **Alt+Right** for
temporary. If a temporary pane is already open, `/temp` focuses it.

Close it with **[X]**, `/temp close`, or the right-sidebar close control.
Closing it cancels its request and deletes its history, without recovery or
trash. Normal exit also discards it. Temporary history does not enter chat
lists, bulk exports, or automatic cross-chat recall.

Explicit `@save` can create a Saved file. Contextual `@remember` and `@memory`
skills work when Memory is available and configured. Other selected skills
keep their normal behavior. Completed explicit saves survive closing the
temporary chat. A skill mention alone does not request a memory write.
Automatic local-media saving is disabled for temporary requests. Closing a chat
cannot undo a completed skill operation or remove data already sent to a provider.

In the main chat, ask **“Read the temp chat”** or **“Read the ephemeral chat”**
to copy its visible text into that request and the main chat's saved history.
The copy includes a live partial response, but not hidden thinking. It is
marked as untrusted reference data and does not itself activate skill commands.
If main is still the source conversation, only new branch messages are copied;
if main has changed to another conversation, the whole temporary history is
copied. This explicit copy is not limited to the automatic recall excerpt size.
It remains in main after the temporary pane closes. `/fork` from the temporary
pane instead creates a persistent continuation in main and leaves the pane open.

### Relevant context from other chats

Before a request, TCUI searches a bounded set of local chat documents, encrypted
when chat protection is selected, for matching words. It can send **up to three excerpts, at most 3,000 characters
total**, to the selected provider. Each excerpt includes its conversation ID,
title, and role and is marked as untrusted evidence, not instructions. This is
automatic and independent of the optional Memory feature: content from another
chat can therefore reach the provider selected for the current chat.

The search excludes the current chat, trash, and duplicate content already in
the request. It examines at most 128 eligible documents, 8 MiB total, and 4,096
directory entries; documents over 1 MiB are skipped. A notice reports unreadable
files or a limited search. Excerpts are request-only: they do not become saved
messages or new memories. Matching is lexical, not a claim of semantic recall
across all history.

### Literal text and contextual memory skills

HTML/XML-like text remains visible in chat. Answer text does not rename
conversations or execute hidden memory directives. Titles come from the first
user message, with a five-word and 16-Unicode-character limit before an
optional ellipsis.

Enable **Settings → Memory → Enable memory** to use memory skills. This also
enables automatic recall for ordinary requests; memory encryption is a separate
choice under `/security`. Select a skill anywhere in your own prompt, or use
`/skill <name> <request>`. Memory uses its own native store, separate from Vault:

```text
This machine is used for local CI @remember that, and the configuration we have set up this session.
This machine runs Ubuntu, but I don't @remember what it is for.
Recover @memory related to this machine.
@recall the saved hardware details for my home server.
/skill recall how this machine was configured
```

`@remember`, `@memory`, and the existing `@memorize` alias use the full prompt
and current conversation. The model selects saving or recall from the meaning
of the request. They do not save the literal text after a marker. If required
facts or the intended machine are unclear, the skill asks before saving.
A plain reply can continue that clarification in the same conversation,
including after restart. A copied question from another chat cannot authorize
the operation.

`@recall <query>` and `/skill recall <query>` retrieve existing memories without
changing them. The host rejects save, removal, and reindex actions for those
invocations, including prompts that also mention another memory skill.
Quoted examples, code, attached documents, and assistant text do not invoke
memory actions. The host reports actual operation results. Normal answer
text cannot execute a memory action.

Contextual saves contain Markdown, user or machine scope, relevant tags, and
a source-conversation title snapshot. Machine records also contain a machine
ID and readable name. The local identity comes from the TCUI execution host;
a remote or discussed machine is not automatically that host. Named targets
use an existing saved identity or a label-derived identity, not verified
remote hardware discovery. Persistent source IDs are retained when available.
A first-turn save can precede chat ID allocation; temporary sources retain a
title without a durable chat ID. Existing unscoped records remain unchanged.

Memory instructions, plans, and saved Markdown have a 1 MiB limit. Contextual notes do not
have the old literal-fact 500-character limit. Empty notes and recognized
credential patterns are rejected. Manual retrieval uses at most four planning
steps, eight search hits per query, and 16,000 characters per tool result;
truncation is reported. Machine/scope filters apply before vector ranking.
Existing synchronization still scans records before a search.

Open the right sidebar with `Ctrl+Right`, then use **Memories → View** to
inspect the note and its stored metadata. **Insert to chat** copies the note
into the next request. The copy remains in chat history after memory deletion.

A requested memory removal or sidebar **Delete** moves the record to Trash and
updates the index. Failures are reported rather than presented as successful
deletion. Neither path erases existing chat copies. Sorting, filters, and further
recall/management work remain under AR-15. Completed source operations are
reported separately from index failures.

Saved records live independently in `$XDG_DATA_HOME/tcui/memories` (normally
`~/.local/share/tcui/memories`). `/security` can protect Memories with the shared
password, independently of chats and Vault. Protected search indexes are
encrypted serialized SQLite on disk and queried in RAM, without plaintext
SQLite sidecars. A corrupt index does not prevent reading or listing source
records; repair is explicit and does not rewrite the sources.

Automatic capture remains removed. Automatic recall still runs for ordinary
prompts when Memory is enabled. Explicit memory tasks use their own bounded
retrieval instead of a second automatic memory injection. There is no
independent automatic-recall switch or multi-device synchronization yet.
See the [actionable review](docs/2026-10-22-actionable-review.md) for the
remaining memory work.

## Settings

`Ctrl+P` opens **Settings search**. With no query it offers **Settings tab**;
typing searches settings and available actions. Selecting a setting opens its
category on the dedicated Settings tab. `/settings` and the launcher open that
tab directly.

Settings are grouped into Appearance, Terminal, Layout, Chat, Web search,
Connections, Models, Local inference, Notifications, Pushover, Files & vaults,
Keyboard, MCP, Privacy, and Memory when that feature is enabled. Controls include
previously hidden sidebar widths, status widgets, local probe timeouts, mobile
notifications, memory limits, and terminal defaults. Provider secrets stay in
their dedicated managers. Inactive legacy fields are not presented as working settings.

Use Tab to switch category/list focus and arrows to navigate. Toggle booleans or
cycle choices directly; Enter edits text and numbers and applies valid values.
Changes still persist individually. The **Save** button and **Ctrl+S** also save
all current settings, including a valid open draft. A failed save keeps the draft
and previous saved value. Enter adds a line in multiline fields; Ctrl+Enter applies
the field. Esc cancels an edit. Global tab/settings shortcuts preserve open drafts.

Settings accept plaintext, not JSON. Provider/model lists and command rules use one
entry per line. Non-command lists preserve spaces; `\n`, `\r`, `\t`, and `\\` encode
literal control characters/backslashes, and `\e` represents an empty entry.
Keyboard actions support capture/reset; bulk mappings use `new_chat=alt+n`.
Status widget rows use `provider 1 1` (widget ID, row, area).
MCP servers use labeled blocks separated by a blank line, for example:

```text
name: local-tools
transport: stdio
command: my-mcp-server
args: '--directory' 'path with spaces' ''
enabled: false
```

The other MCP field is `url:` for HTTP servers. Omit absent optional fields;
`args:` with no value means an empty argument list. MCP fields support `\\`, `\n`,
`\r`, and `\t` escapes. Saving configuration does not run server commands.

Terminal placement updates the current split immediately. Shell/directory and
provider/model defaults apply to new sessions or conversations; existing sessions
keep their state. Recovery retention changes policy for the next startup recovery
pass, not an immediate purge.

## Commands and terminal tabs

### Approved direct commands

Mention `@command` in your request to let the agent choose and run approved
programs, for example `Please @command check the current UTC date`.
`/skill command ...` is also supported. Configure **Settings → Terminal → Command
whitelist** and **Command blacklist**, with one rule per line. An empty whitelist
denies all direct launches. Blacklist matches always win, including canonical
symlink targets. Each rule is an executable name or absolute path, optionally
followed by an exact literal argument prefix. Quote values that contain spaces.

`curl` permits any curl arguments. `curl -H` permits `curl -H "X-Name: value" URL`,
but not `curl URL`, `curl --header ...`, or `curl -s -H ...`. Rules do not normalize
option aliases or match flags at arbitrary positions. A bare blacklist rule denies
every argument list for that executable; a constrained rule denies matching prefixes.
A whitelisted name approves its canonical PATH executable, not every file with the
same basename. Native Pushover calls must match their complete curl arguments too.

The agent receives the conversation, current request, and invoked skill bodies.
It chooses executables and literal arguments, observes actual tool results, and
can continue with another action. TCUI does not interpret shell syntax: `$HOME`,
`$(...)`, pipes, and semicolons remain literal arguments. Optional standard input
supports private request bodies. History, attachments, quoted examples, and
assistant output cannot authorize commands by themselves.

Arguments are bounded, each output stream is limited to 64 KiB, and execution
times out after 30 seconds. Cancelling a request terminates its Unix process
group. Children receive a restricted environment rather than ambient API keys or
shell startup hooks. Command output becomes untrusted chat/model context: do not
approve helpers that print secrets.

This is an executable allowlist, **not a sandbox**. Trust the approved executable,
its location, and its contents. Scripts, interpreters, wrappers, and powerful
utilities retain their own capabilities and can launch other programs. The
policy governs TCUI's direct command and Pushover launches, not commands you
manually approve in an interactive terminal. Skills can be composed in one
request, including `@remindme` and `@ping-user`; no pipe-delimited user grammar
or directive ordering is required.

### Reviewed shell blocks

Completed assistant command blocks use an explicit fence such as `bash tcui-run`.
Supported shells are `sh`, `bash`, `shell`, `zsh`, and `fish`; `tcui-run` is required.
Fences must be top-level, unindented, closed, and nonempty. Ordinary shell examples,
quotations, script-file contents, and unfinished or streaming blocks are not actionable.

Command blocks show their shell and mark visually wrapped lines. Each has a
**top-right copy icon** and a **bottom-left
`>_ Run in terminal` control**. Copy preserves the exact command payload, including
newlines, without fences, prose, or display wrapping. Run opens review for **only
the clicked block**; receiving a response or opening review never executes it.
The full-answer copy icon remains separate.

Shell commands and arguments use a contrasting theme color and bold text.
Recognized output headings, exit-status labels, and control-flow scaffolding keep
the normal code color. Ordinary `echo`/`printf` work, redirections, and pipelines
remain emphasized. Colors aid reading, not safety assessment; they never change
what is copied or approved. Unsupported or oversized syntax remains readable.

`assets/TCUI.md` asks for short output headings followed directly by commands,
keeping related steps in one paste-ready block. It discourages unnecessary
conditionals, wrappers, functions, and exit-status boilerplate; use them only when
the task needs them. Journal/systemd queries retain supported `--no-pager` options.
Independent diagnostics may continue after a failure; dependent changes must stop
when prerequisites fail. Warnings and commands being discussed rather than
recommended stay inline, not in runnable blocks.

- The clicked block starts selected. Space or its checkbox toggles selection;
  PageUp/PageDown scroll the payload.
- **Copy** or Ctrl+C copies selected commands, not surrounding prose.
- **Run** or Ctrl+R explicitly approves the displayed commands in a **new terminal**.
  The fence interpreter is respected.
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

Choose password-encrypted or explicit plaintext SQLite storage for each
connection. Encrypted and plaintext provider/search records, including OAuth
tokens, can coexist; encrypted entries carry an encrypted label. Lookups use
exact connection IDs, and saving cannot silently replace an opposite-storage
record. `/vault` manages **tcui-credentials**, including machine passwords and
multiline SSH private keys, which always remain encrypted.

The `/vault` storage default applies to new records, not a silent bulk conversion.
Changing the credentials checkbox in `/security` intentionally converts provider
and search records and sets that default; leaving it unchanged preserves
per-record exceptions. If the first encrypted connection follows plaintext-only
setup, enroll the shared password, then reopen `/connect`. `/unlock` unlocks all
selected domains. Plaintext credentials and pre-conversion backups remain
readable to anyone with filesystem access. There is no password recovery.

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

Response notifications still occur only while TCUI is unfocused. Desktop helper
errors are captured rather than printed over the TUI, and a failed desktop backend
is disabled for the process lifetime. Set `desktop = false` under `[notifications]`
in `config.toml` to disable automatic desktop delivery explicitly. Configured
Pushover delivery is independent.

Detached SSH reminders cannot notify a terminal that is no longer attached.
They preserve Herdr routing context when available; otherwise configure Pushover
for delivery after disconnecting.

### Pushover

TCUI sends Pushover HTTPS requests directly through `curl`; no notification script
or Python installation is needed. Install curl on the machine or Toolbox where
TCUI runs, then approve `curl` in **Settings → Terminal → Command whitelist**.
The blacklist still overrides approval. Approving curl also permits explicit
`@command curl` requests; command approval is not a filesystem or network sandbox.

Visit [pushover.net](https://pushover.net), download the Pushover mobile app, and
obtain your user key and an application API token. Pushover requires both; TCUI
does not ship a shared application token. Additional named application tokens
are optional.

Open **Settings → Pushover**, enable delivery, and select **Use TCUI-keystore
credentials**. **Set / replace user key (masked)** and **Set / replace selected
app token (masked)** open masked credential entry. **Encrypt new / replacement
keys** defaults to enabled; turn it off only to explicitly store the next key in
plaintext SQLite. Changing this option does not convert existing keys. Replacing
an encrypted key requires unlocking it, even when its replacement is plaintext.

The default application name is `tcui`. To add another token, change **Selected
application name**, then store that application's token. Other saved application
tokens remain unchanged. Names use 1–64 lowercase ASCII letters, digits, hyphens
or underscores. Encrypted records require unlock; explicit plaintext Pushover
records remain readable while the credential vault is locked.

```toml
[commands]
whitelist = ["curl"]
blacklist = []

[notifications.pushover]
enabled = true
use_vault = true
encrypt_credentials = true
application = "tcui"
```

Alternatively, turn off TCUI-keystore credentials and set **Environment credential file**
to an absolute, user-owned, private `0600` literal `KEY=value` file, or supply
environment variables. TCUI does not automatically load the project's `.env`.
Use `PUSHOVER_USER_KEY` for the user key. For the default `tcui` application,
`PUSHOVER_TCUI_APP_TOKEN` takes precedence over `PUSHOVER_APP_TOKEN`. For any named
application, supply `PUSHOVER_APP_TOKEN`; the TCUI-specific token is never used.
In environment/file mode this generic token represents the currently selected
application, not a stored mapping of multiple application profiles. Environment
values override corresponding file values. TCUI-keystore mode never falls back
to files or environment when a selected key is locked or missing.

For immediate delivery, ask `@ping-user Tell me the backup is ready`. The agent
can use the local Pushover tool without reading raw keys, or explicitly request
Pushover credentials to construct an approved curl call. **Reading raw keys sends
them to the configured AI provider.** This capability exposes only Pushover keys,
not other environment-file values, provider credentials, or the master password.
Private credential observations are excluded from ordinary chat context, and
known keys are redacted from displayed results and sensitive provider diagnostics.

Curl receives the JSON request through standard input, not secret-bearing command
arguments or environment variables. TCUI reports native delivery success only
after curl succeeds and the Pushover API returns success.

### Scheduled notifications

Invoke skills naturally anywhere in the current request, for example:

- `@remindme via @ping-user in 20 min to look into those companies`
- `Please @ping-user in 60 minutes to review the backup`
- `@schedule a reminder for tomorrow at noon via @ping-user`
- `@remindme show my pending reminders`
- `Please @remindme cancel the reminder we just created`

The agent reads the full conversation and invoked skills, interprets timing and
message content, and calls the reminder tool. No leading directive, fixed duration
grammar, or `|` separator is required. Future Pushover requests use scheduling,
not an immediate notification. The agent observes registration/cancellation
results before reporting success; completed-action receipts remain in the chat
even if a later model response fails.

Pushover-enabled reminders capture credentials from the configured source during
scheduling. Missing keys, a locked encrypted record, or denied curl fail rather
than silently creating a local-only timer when Pushover was requested.
Relative, absolute, daily, weekly, and systemd-calendar schedules are supported.
Reminders also attempt configured desktop delivery independently.

Timers use the host's user systemd manager. Inside Toolbox, TCUI registers through
`flatpak-spawn --host --watch-bus` and re-enters the originating container for delivery.
Keep that container, TCUI executable path, and curl available. Closing TCUI does
not cancel a timer, but these transient timers do not promise reboot survival;
the user manager must remain running after logout.

**Pending Pushover jobs contain plaintext messages and credentials**, even with
app/chat/vault encryption enabled. Snapshots live in private `0600` files under
`$XDG_DATA_HOME/tcui/reminder-notifications/`, normally
`~/.local/share/tcui/reminder-notifications/`.
They intentionally work without unlocking TCUI again. Locking the vault,
changing its password, or replacing vault keys does not revoke captured
credentials: cancel existing jobs first. The enabled flag and command policy
are checked again at delivery. Registered one-shot snapshots and records are
removed after the delivery attempt, including failure; there is no automatic
retry. A service post-stop hook also attempts cleanup if the callback fails before
delivery. One-shot timers and services are configured for collection after success
or failure; completion disarms the timer without waiting on its own stop hook.
Recurring snapshots remain until cancellation.

Cleanup requires the TCUI executable/container to remain available. Cleanup errors
are reported, and unresolved registration is retained when possible so cancellation
can retry. Missing or unreadable metadata plus unknown cadence is not permission
to delete a possibly recurring snapshot: restore its metadata or recover/remove
it explicitly.

Reminder metadata uses a separate private `reminder.key`, not the password-protected
chat key. Back up `reminders.toml` together with `reminder.key` when moving reminders.
Existing legacy records retain their ciphertext and migrate their original
`chat.key` without replacement. Missing or incompatible keys require restoration;
TCUI does not generate a replacement over unreadable reminder data.

## Vault and Memories

The new-tab launcher contains working **Vault**, **Memories**, **Settings**, and
**Terminal** views. There are no external Vault profiles or unfinished feature
cards. Existing external folders are not imported, searched, changed, or deleted.
Configuration migration keeps a backup before it removes obsolete profile settings.

The two internal stores are separate:

- **Vault:** `$XDG_DATA_HOME/tcui/vault`, normally `~/.local/share/tcui/vault`.
  It holds saved text, binary files, downloads, and files related to chats.
- **Memories:** `$XDG_DATA_HOME/tcui/memories`. Native `.tcui-memory` records
  retain their IDs, source details, scope, and creation metadata when edited.

Open either view from `+`. Select an entry and use `e` to edit, `c` to copy,
`d` to move it to Trash, `+` to insert context, or `i` to import or export.
Memories also has clickable controls. Plaintext Vault text can use the configured
terminal editor. Protected Vault files, binary files, and Memories use an in-app
editor; decrypted edits do not require a plaintext temporary file. Binary edits
use base64. The in-app editor supports up to 64 MiB of decoded content.
The in-app editor checks the original content digest before saving. A concurrent change leaves both the
stored file and the unsaved editor buffer intact.

The right sidebar has **Chat Artifacts**, **Memories**, and **Vault**.
Chat Artifacts are conversation-scoped links to canonical files. Deleting a
conversation removes only its links. Deleting a Chat Artifact removes its body.
Use its **Vault** action to make an independent, collision-safe Vault snapshot
that survives deletion of the Chat Artifact. Vault and Memory deletion uses
internal Trash. Clipboard actions send plaintext text, or base64 for binary data;
terminal clipboard requests have no acceptance acknowledgement.

Native `@vault` operations address only internal paths:

```text
@vault list my notes
@vault read projects/status.md
@vault search for "renewal date"
```

Mutations require a preview and explicit confirmation. An explicit legacy Saved
directory remains a migration source. A missing legacy mount blocks migration
instead of being recreated; remount it and retry with the same password.

### Context snapshots

The sidebar context control or `+` inserts a removable **`<Memory #432>`** or
**`<Vault item #32>`** chip. Sending expands its captured content between visible
resource tags in the user message. Those tags and content follow the destination
chat's persistence policy. Later source edits do not change an inserted snapshot.
Source text cannot select host commands such as `@save` or `@schedule`; only the
typed request can do that.

TCUI asks before placing protected content in an unencrypted chat. Sending also
shares that content with the selected provider. Binary context uses lossless
base64. Context is limited to 256 KiB per item after encoding and 1 MiB in total,
including resource tags.

### Move data to another instance

Close TCUI before copying native folders, or use `i` → **Export portable**.
Both plaintext and encrypted files can move to another machine. New protected
Vault and Memory files use self-contained `TCUI-PORTABLE` version 2 envelopes.
They need the source password, not the source provider credentials or database.

Copy native data into the new instance's corresponding folder, or choose a source
file or folder through `i`:

- **Add / adopt:** authenticate copied encrypted files, then adopt them in place
  or add them to the current store. Name or ID conflicts keep both versions.
- **Replace dataset:** retain the old dataset and its identity metadata in a
  recovery backup before replacing it.
- **Restore backup:** restore that dataset and retain the displaced data separately.
- **Export plaintext:** require explicit consent before writing decrypted data
  outside TCUI protection.

TCUI recognizes copied encrypted files and asks for the source password. It then
uses the destination instance's password. Imported encrypted data stays encrypted
unless the user explicitly confirms a plaintext protection choice. Provider
credentials do not transfer. Numeric context IDs belong to the local catalog;
imports preserve source IDs when they are available and do not conflict.

External import/export is limited to 64 MiB per file and 256 MiB per operation.
Native adoption and retained backups support 1 GiB per stored file with no total
byte cap. File-count and path-depth limits still apply. Legacy V1 files need a
source-side upgrade with the original wrapping metadata; old `enc:v1:` Memory
records also need the original `chat.key`. A password alone cannot recover them.

See the [Vault guide](skills/vault/SKILL.md) for native operations, workspace
navigation, move recovery, and limits.

## Install

Releases target **Linux x86-64 with glibc 2.34 or newer**, plus the GCC support
runtime and CA certificates. CI runs on Ubuntu 26.04, but release builds explicitly
target the older ABI instead of inheriting the runner's libc. Native packages and
archives remain the distribution formats. Toolbx is a first-class way to build
and run TCUI, not a requirement.

The baseline is a minimum ABI, not a bundled libc or an exact runtime version pin.
The binary uses the installed distro's glibc, including newer implementations.
Keep the distro supported and its security updates installed: vendors
[backport security fixes](https://access.redhat.com/security/updates/backporting)
without necessarily changing the upstream version number.
Running a prebuilt binary does not require Rust, Cargo, a C/C++ compiler, Zig, mise,
or a container. Optional integrations still need their external commands/services.

GNU/glibc is the release and required CI target. Static musl builds are available
from source without a second required CI matrix. macOS remains experimental;
see the source-build instructions below.

Each release contains:

- `tcui_<version>_amd64.deb` and `tcui-<version>-1.x86_64.rpm`
- `tcui-x86_64-unknown-linux-gnu.tar.gz` with a prebuilt binary, installer, and licenses
- `PKGBUILD` for Arch's `tcui-bin`, with release-specific checksums
- `tcui-<version>-source.tar.gz` from the exact tagged commit
- `install.sh`, `SHA256SUMS`, `LICENSE`, and `potion-base-8M-LICENSE`

### Local user install and uninstall

Download and inspect the installer, then run it without sudo:

```bash
curl -fL https://github.com/petterssonjonas/tcui/releases/latest/download/install.sh -o install.sh
```

Inspect the downloaded script before executing it:

```bash
bash install.sh
```

The installer downloads the tarball and requires a matching `SHA256SUMS` entry
before replacing the binary. It never invokes a package manager. Uninstall removes
only the installed binary, leaving configuration, chats, credentials, and directories.
Keep the downloaded script; uninstall later with `bash install.sh uninstall`.

| Variable | Default | Purpose |
| --- | --- | --- |
| `TCUI_VERSION` | `latest` | Release tag, with or without `v`. |
| `TCUI_BIN_DIR` | `$HOME/.local/bin` | Absolute, normalized, symlink-free destination. Add it to `PATH`. |
| `TCUI_REPO` | `petterssonjonas/tcui` | Release repository, including forks. |

For offline installation, download the tarball and `SHA256SUMS` from the **same
release**, verify the archive, then extract it into an empty directory and run
its `install.sh`. A sibling `tcui` binary selects offline mode and is trusted as
already verified; `TCUI_VERSION` and `TCUI_REPO` do not override that binary.

```bash
(
  set -euo pipefail
  grep '  tcui-x86_64-unknown-linux-gnu.tar.gz$' SHA256SUMS | sha256sum --check --strict
  mkdir tcui-release
  tar -xzf tcui-x86_64-unknown-linux-gnu.tar.gz -C tcui-release
  bash tcui-release/install.sh
)
```

Checksums detect corruption; they do not authenticate a compromised publisher.
Inspect downloaded scripts and recipes before executing them.

### Native packages and Arch

Install the downloaded `.deb` with `sudo apt install ./tcui_<version>_amd64.deb`,
or the `.rpm` with `sudo dnf install ./tcui-<version>-1.x86_64.rpm`.
Use the same package manager to upgrade or remove a system installation.
The local installer/uninstaller does not manage system packages.

On Arch, download `PKGBUILD` from the selected release, inspect it, then run
`makepkg -si`. It downloads and verifies the matching x86-64 binary and both
licenses. `packaging/PKGBUILD` is a template, not a directly usable AUR recipe;
the release pipeline generates the usable file. Nothing is published to AUR
automatically.

### Build from source on bare metal

Use a complete checkout or the release's source archive, including `Cargo.lock`
and the bundled model assets. Install [mise](https://mise.jdx.dev/getting-started.html)
and the native SDK for the build machine:

| Linux distribution | Native build packages |
| --- | --- |
| Ubuntu / Debian | `gcc`, `binutils`, `libc6-dev` |
| Fedora / RHEL-compatible | `gcc`, `binutils`, `glibc-devel` |
| Arch | `gcc`, `binutils`, `glibc` |

Rust/Cargo are versioned through mise. These system compiler/linker/header packages
are native SDK exceptions, not TCUI runtime compiler requirements. Bash, CA
certificates and download tools are also needed for setup. No `just`, C++ compiler,
separate SQLite installation, or OpenSSL development package is required.

From the source directory:

```bash
mise install rust
mise exec rust -- cargo build --locked --release
./target/release/tcui
```

Naming `rust` in `mise exec` limits automatic tool installation to the source-build
toolchain. Bare `mise exec -- ...` can otherwise install every configured tool,
including release packaging and CI tools that an ordinary source build does not need.

This ordinary GNU build uses the build machine's libc; it does **not** promise the
release's minimum ABI. Arch users can compile this way without using the binary
PKGBUILD. To install a source build locally, copy the executable to a directory
you control in `PATH`; remove that executable to uninstall. Keep configuration
and credentials unless you explicitly intend to delete your data.

#### GNU build with the glibc 2.34 release baseline

On Linux x86-64, use the pinned Zig cross-linker rather than downgrading host glibc.
Zig and `cargo-zigbuild` are build tools installed in mise's user-local tool
directory, not system packages or TCUI runtime dependencies. Cargo does not fetch
Zig by itself; the following mise command installs both pinned tools. No Zig
application source is involved, and this does not replace the system's glibc.
Python 3.11+ and `readelf` from binutils are required for the artifact check.

```bash
mise install rust zig cargo:cargo-zigbuild
mise exec rust zig cargo:cargo-zigbuild -- bash scripts/build-release.sh
./target/x86_64-unknown-linux-gnu/release/tcui
```

The script sets the x86-64 CPU baseline and rejects inherited compiler overrides.
It checks the executable's loader, shared libraries, and required symbol versions;
optional weak symbols are not mistaken for mandatory newer libc dependencies.
Full CI also runs the packaged executable in a pinned glibc 2.34 environment.
`CARGO_TARGET_DIR` can select another writable build/output directory.

#### Static musl build

Build this separately from the GNU release. The included header supplies a missing
`sys/types.h` include in sqlite-vec's C build; it does not disable vector memory.

```bash
mise install rust zig cargo:cargo-zigbuild
mise exec rust -- rustup target add --toolchain 1.99.1 x86_64-unknown-linux-musl
CFLAGS_x86_64_unknown_linux_musl="-include $PWD/scripts/musl-headers.h" \
  mise exec rust zig cargo:cargo-zigbuild -- cargo zigbuild --locked --release --all-features \
    --target x86_64-unknown-linux-musl
./target/x86_64-unknown-linux-musl/release/tcui
```

The static executable does not require the destination system to install musl or
glibc. A compatible Linux kernel/CPU, certificates, and any external commands you
use are still needed. DNS/NSS integration can differ from glibc. Rebuild to pick up
bundled runtime security fixes. Musl builds intentionally refuse `tcui upgrade`:
published upgrade assets are GNU binaries, so update musl builds from source.

#### macOS build attempt

On a Mac, install Apple's Xcode Command Line Tools (`xcode-select --install`) and
mise. The Apple SDK/compiler is a platform SDK exception to mise tool management.
From the source directory:

```bash
mise install rust
mise exec rust -- cargo build --locked --release
./target/release/tcui
```

Native targets are `aarch64-apple-darwin` (Apple Silicon) and
`x86_64-apple-darwin` (Intel). The Linux-to-Apple-Silicon cross-build reached the final
link, then failed because the Apple SDK/frameworks were unavailable; no native Mac
run has been verified. Cross-compilation needs a legitimately obtained SDK, not
just the Rust target. Systemd reminders have no macOS backend, native Notification
Center integration is not implemented, and the updater/installer publish only
GNU/Linux x86-64 artifacts. A successful compile alone does not establish full
macOS support.

### Running in Toolbx or Distrobox

Use a container for an integrated workspace or to run on an older host libc
without replacing it. The host must support the container engine and image;
this does not fix an unsupported CPU architecture or an incompatible kernel.
On managed servers, obtain permission to use rootless containers first.

Choose **one** of these environments. Reuse an existing `tcui` container rather
than recreating it:

```bash
# Toolbx: commands run on the host.
toolbox create --image registry.fedoraproject.org/fedora-toolbox:45 tcui
toolbox enter tcui
```

```bash
# Distrobox: commands run on the host.
distrobox create --name tcui --image docker.io/library/ubuntu:26.04
distrobox enter tcui
```

Inside the container, ensure Bash, curl, tar, CA certificates, and the GCC support
runtime are available from the image's package manager (`libgcc` on Fedora,
`libgcc-s1` on Ubuntu). Download and inspect `install.sh`, but do not run the
ordinary host-install command above. Use a separate destination so that an
incompatible binary does not replace a host-native installation:

```bash
# Inside the chosen container, after inspecting the downloaded installer.
TCUI_BIN_DIR="$HOME/.local/share/tcui-container/bin" bash install.sh
"$HOME/.local/share/tcui-container/bin/tcui"
```

Subsequent launches from the host must also enter the chosen container:

```bash
# Toolbx:
toolbox run --container tcui "$HOME/.local/share/tcui-container/bin/tcui"
# Or Distrobox:
distrobox enter --name tcui -- "$HOME/.local/share/tcui-container/bin/tcui"
```

Uninstall inside that container with
`TCUI_BIN_DIR="$HOME/.local/share/tcui-container/bin" bash install.sh uninstall`.
Configuration and credentials remain in the shared home directory.

**These environments are not security sandboxes.** They integrate with the host,
including its home directory and potentially its credentials and sockets.
TCUI's local commands and embedded terminals run inside the container by default;
package and service commands do not automatically manage the host. You can copy a
suggested command into a separate host terminal instead: using TCUI's embedded
terminal is optional. For execution from TCUI, use an explicit SSH connection or a
deliberate host-execution mechanism, and confirm which machine receives the command.

Scheduled reminders and desktop notifications need separate integration checks:
host systemd may otherwise try to execute the container's binary using the
incompatible host runtime. These mutable environments also need package updates;
pulling a newer image does not update an existing container. Image tags and
subsequent package changes are not a reproducibility guarantee.

See the upstream [Toolbx documentation](https://containertoolbx.org/doc/) and
[Distrobox documentation](https://distrobox.it/usage/distrobox-create/) for engine
requirements, host integration, and image maintenance.

### Compiling in Toolbx or Distrobox

Create or enter an environment as above; compilation does not require installing
a released TCUI binary first. Install mise and the native SDK packages **inside**
that environment, then follow the bare-metal GNU or musl build instructions from
the source directory in the shared home. For example, after installing the Rust
pin inside `tcui`, run from the checkout on the host:

```bash
toolbox run --container tcui mise exec rust -- cargo build --locked --release
toolbox run --container tcui ./target/release/tcui
```

For Distrobox, use `distrobox enter --name tcui --` in place of
`toolbox run --container tcui`. Ordinary builds should continue running inside
their build environment: compiling against newer container glibc does not make
the result compatible with an older host. Use the explicit glibc 2.34 or static
musl procedure when building for another machine, and verify there before relying
on it. Contributors and agents still follow the `tcui` Toolbx workflow in
`AGENTS.md`; bare-metal instructions are available for end-user source builds.

For system-specific help, provide `/etc/os-release`, `uname -m`, `uname -r`, and
`getconf GNU_LIBC_VERSION` output (or its error on a non-glibc system). A useful
request to your administrator or AI assistant is:

> Recommend the least-invasive supported way to run or build TCUI on this system.
> Check CPU, libc, kernel, container permissions, and where commands will execute.
> Keep the host libc unchanged, use verified release artifacts, and explain any
> package installation or privilege requirement before proceeding.

Do not share credentials, private keys, or an unfiltered environment dump.

## Contributing and releases

Read [AGENTS.md](AGENTS.md).
Feature PRs squash into `development`; reviewed promotions use a merge commit
into `main`. Auto-merge is never enabled.

Codegraph and Context7 are development tools, not application dependencies.
Initialize a checkout with `codegraph init .`, then refresh with `codegraph sync .`.
The index remains ignored and local. Use Context7 for version-appropriate library
documentation and the relevant installed Rust skills for Rust work and review.

Local development runs inside `toolbox enter tcui`. Rust is pinned to **1.99.1**
in both `mise.toml` and `rust-toolchain.toml`; activate mise in that shell or prefix
Rust commands with `mise exec rust --`. Install development tools with `mise use <tool>@<version>`
from the project directory, rather than installing an unversioned host or distro CLI.

CI prerequisites: the pinned Rust toolchain, a C compiler and linker, Python 3.11+,
Bash, curl, tar, Git, and standard Linux tools. `scripts/setup-ci.sh` installs the
requested tier's tools from `mise.toml`; local and GitHub CI use the same pins.
Full validation adds Zig/cargo-zigbuild, cargo-deb, and cargo-generate-rpm.
Package inspection needs binutils, `dpkg-deb`, `rpm`, `rpm2cpio`, and `cpio`;
these native package-system tools may require a distro-package exception.
CI disables implicit tool installation during execution so that the selected
setup tier, not the complete tool manifest, determines what gets installed.

TCUI itself is Rust. SQLite/sqlite-vec, Tree-sitter and AWS-LC still contain native
C code; BLAKE3 retains its optimized assembly backend. The memory tokenizer uses
Rust regex and disables unused C++ training code. The Linux build does not require
a C++ compiler or `libstdc++`; replacing the database or TLS provider solely to
remove C would require a separate compatibility/security evaluation.
Tokio's virtual-clock utilities are enabled for test targets, not ordinary builds
(including ordinary debug builds).

The GCC support runtime (`libgcc_s.so.1`) is not a compiler. Ordinary GNU builds
link it directly; the Zig release currently does not. Packages retain this
standard runtime, which is also a dependency of Rocky 9/Fedora glibc.

Dependency checks use `mise exec rust cargo:cargo-audit -- cargo audit` (pinned in `mise.toml`).

```bash
bash scripts/setup-ci.sh development
MISE_EXEC_AUTO_INSTALL=false mise exec -- bash scripts/ci.sh development
# Full validation requires a clean committed checkout and absent/empty target/dist:
bash scripts/setup-ci.sh full
MISE_EXEC_AUTO_INSTALL=false mise exec -- bash scripts/ci.sh full
# Separate oldest-runtime check needs Docker, or TCUI_CONTAINER_ENGINE=podman:
bash scripts/smoke-oldest-runtime.sh
```

Development runs Rust lint/tests/CLI smoke plus installer and workflow checks.
Tests use temporary HOME/XDG directories while preserving Rust's tool and cache locations.
Environment-mutating tests retain the shared environment lock until cleanup.
Run their async work with `Runtime::block_on` from a synchronous test, keeping
lock acquisition outside the async runtime.

The `auth_cli` integration suite clears the inherited environment and uses private
HOME/XDG directories, temporary provider/browser stand-ins, and loopback HTTP
fixtures. It checks credential storage, cancellation, and password-descriptor
handling without live provider accounts. Run this suite in the default debug
profile: its local-endpoint overrides are not available in release binaries.
CI isolates test data; it does not currently enforce an outbound-network deny rule.

Main adds the no-default-feature configuration, rustdoc, the glibc 2.34 release
build and ABI gate, packaging, and native-payload/Arch-recipe/install smoke.
Ubuntu 26.04 full CI/release jobs additionally run the packaged binary in a pinned
Rocky Linux 9 container. That checks the old userspace ABI, not an old host kernel.
Musl has source-build instructions rather than a second required CI matrix.
Full validation produces `${CARGO_TARGET_DIR:-target}/dist`; move previous outputs
aside before another full run. Local Toolbx validation does not require nesting
containers; run the separate oldest-runtime check where an engine is available.
Both branches also run `bash scripts/scan-secrets.sh`: it downloads a checksum-pinned
Gitleaks CLI and scans full reachable Git history, including merge resolutions.
A shallow checkout is rejected. This scan checks committed history, not uncommitted files.

Only an explicit release instruction authorizes tagging and publication. After
the version commit reaches `main`, create a stable `vX.Y.Z` tag on that commit
and manually run `release.yml` **on main**, supplying `tag=vX.Y.Z` (Actions UI or
`gh workflow run release.yml --ref main -f tag=vX.Y.Z`). The workflow verifies
ancestry and version, runs full CI on the exact tag, and creates the GitHub
Release with all assets. Merges and tag pushes alone do not publish anything.
Before publication, verify GitHub protections require reviews and passing checks
on both branches, block direct pushes, and restrict the `release` environment to
`main` with owner approval. Workflow files do not configure those remote safeguards.

## License

TCUI is licensed under GPL-3.0-only. Bundled `potion-base-8M` model assets remain under their
upstream MIT license; the release artifacts ship that license alongside the binary packages.
