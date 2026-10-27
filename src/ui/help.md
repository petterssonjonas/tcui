# Help

Welcome to **TCUI**, a terminal helper for commands, troubleshooting, and scripts using local or hosted LLM providers.

## Available commands

Type any slash command in the message box:

- `/help` — show this help
- `/keybinds` — show keyboard shortcuts
- `/fork` — open a persistent continuation named `Fork of <title>`
- `/temp` — open an ephemeral fork in the Wide right sidebar
- `/temp close` — cancel its request and delete the ephemeral conversation
- `/cancel` — stop this chat's response and keep its partial text
- `/steer <instruction>` — request live steering; current connections report that it is unsupported and keep the draft
- `/settings` — open the dedicated Settings tab
- `/security` — choose app entry password and at-rest encryption independently
- `/unlock` — unlock all protected data with the shared password
- `/lock` — save/cancel active requests, close terminals, clear application caches and lock
- `/terminal` — open a persistent terminal session
- `/providers`, `/connect` — manage added providers, choose a preset or add a custom endpoint
- `/vault` — unlock, initialize or lock the credential vault; choose credential storage; manage machine passwords and SSH keys
- `/vault-search <query>` — search the internal Vault
- `/model <id>` — select a model ID manually, including custom endpoints without a model-list API
- `/theme` — change the theme (or `/theme <name>` to apply one directly)
- `/mcp` — open the MCP servers popup
- `/skills` — open the skills popup
- `/skill <name> <request>` — invoke an installed skill with the full request
- `/web` — toggle automatic web search before each chat message
- `/quit`, `/exit`, `/q` — quit TCUI

## Usage

- Press `Ctrl+P` to search settings and actions. With no query, choose **Settings tab**.
  Search results navigate to the selected setting; the popup does not edit values.
  Changes persist individually. Click **Save** or press **Ctrl+S** to save all current
  settings and a valid open draft. Failed saves retain the draft. In multiline
  editors, Enter adds a line and Ctrl+Enter applies; Esc cancels.
  Settings use plaintext: lists/rules have one entry per line; status widgets use
  `provider 1 1`; bulk shortcuts use `new_chat=alt+n`. Non-command list escapes are
  `\\`, `\n`, `\r`, `\t`, and `\e` for an empty entry. MCP uses labeled fields
  `name:`, `transport:`, `command:`, `args:`, `url:`, and `enabled:`, with a blank line
  between servers. Quote literal arguments with spaces; MCP supports text escapes.
  In Keyboard settings, Enter captures a binding and Backspace resets it.
- Click the **provider** or **model** labels in the status bar to change
  selectors.
- Settings groups include terminal defaults, appearance, layout, chat, web search,
  connections, local inference, notifications, files/vaults, keyboard, MCP, privacy, and memory.
- Run `/keybinds` to see all keyboard shortcuts, including how to rebind them.
- Drag with the left mouse button and release to copy selected visible text,
  including text in credential windows. Masked fields only expose their visible mask.
- On an OAuth sign-in screen, press `c` to copy the complete sign-in URL without
  display line breaks. Your clipboard may retain this temporary URL.
- OAuth works over SSH without a browser on the app host. Open the copied URL
  on your own device, then paste the complete final response URL into TCUI and
  press Enter. Press `p` to open the masked response field manually. A browser
  localhost connection error does not prevent pasting its address.
- Outside command review, `Ctrl+R` refreshes provider model catalogs. OpenAI OAuth
  includes subscription-only models and replaces obsolete selections after refresh.
- Click the copy icon at the top right of an assistant response to copy its full answer.
- The copy icon uses Font Awesome/Nerd Fonts. If it is missing in your font, set
  `TCUI_COPY_ICON=unicode` for `⧉`, or `TCUI_COPY_ICON=ascii` for `[c]`, before launch.
- Markdown tables render with borders, alignment, and wrapped cells. Fenced
  `mermaid` flowcharts and graphs render as connected terminal diagrams.
  Cycles and crowded graphs retain boxes, continuous arrows, and inline labels.
  Mermaid `xychart-beta` renders vertical bar/line charts; `pie` renders colored
  sectors with values and percentages. All work locally and over SSH without a
  browser. Examples are included in the model instructions (`assets/TCUI.md`).
  Unsupported, incomplete, or oversized diagrams retain a readable source view.
- Text-box borders stay aligned when resized. Long code lines wrap instead of
  disappearing; the response copy button preserves the original source.
- Notifications use Herdr, a reachable local desktop service, or supported
  Kitty/Ghostty/WezTerm terminal delivery over SSH. Unsupported routes are off;
  SSH never sends desktop notifications to the server's D-Bus.
- Optional Pushover delivery uses curl directly. Configure encrypted user and
  application keys in Settings → Pushover, and approve curl in Settings → Terminal.
  A locked vault never falls back to plaintext credentials.

## Application protection

First interactive launch asks which data to protect. The startup screen also
appears when a shared password or protection is configured. Unlock with the
existing password, select additional sections, and save without entering that
password again. Unlock stays on the selection screen; **Continue to app** opens
the application. Reopen protection through `/security` or **Settings → Privacy →
Application protection**.

Credentials, chats, memories and the internal vault are independent choices.
Unchecked data remains plaintext; an entry password alone does not encrypt it.
Protected chats or an entry password require unlock before application stores
or chat titles load. Other protected sections can stay locked while you continue
to unprotected parts. Cancelling an unlock returns to protection options;
cancelling the startup screen exits.

After unlocking, **Change shared password** sets and confirms a new password
without changing protection selections or explicit plaintext connections.
Previously exported copies keep their old password. Changing the password
rewraps the data master; it does not revoke a data key already copied or extracted.
There is no forgotten-password recovery.

Changing protection or locking cancels and saves active requests, closes
terminals and reconstructs the application without its old plaintext caches.
Inserted documents require discard confirmation before restarting; cancelling
that confirmation preserves the current session and pending documents.
If setup or a password change is interrupted, use **Unlock / recover** on the
protection screen. Recovery can request the new password after the password
change commits. Application data stays unavailable until recovery succeeds.

Headless content commands require the same session unlock. Supply a password
line through redirected `--unlock-password-stdin`, never through arguments or
environment variables. For `memory-mcp`, stdin belongs to JSON-RPC: use a
dedicated inherited descriptor with `--unlock-password-fd N` (N ≥ 3).

## Vault and Memories

Open these separate internal stores from the `+` tab launcher. Select an entry:
`e` edits, `c` copies, `d` moves to Trash, `+` inserts context, and `i` opens
import/export. Protected and binary Vault files use an in-app editor. Binary
content uses base64. Saving rejects stale edits and keeps the unsaved buffer.

The right sidebar has Chat Artifacts, Memories, and Vault. The Chat Artifact
**Vault** action keeps an independent copy. Memory and Vault context chips use
`<Memory #432>` and `<Vault item #32>`; sending includes their captured content.
TCUI asks before protected content enters plaintext chat history.

Portable exports and copied native folders can move to another instance. TCUI
asks for the source password when needed, then uses the destination password.
Add keeps conflicts; Replace retains a recovery backup; Restore retains the
displaced data. Plaintext export needs explicit consent. Legacy encrypted files
must first be upgraded on their source instance with its original key metadata.

## Conversation ownership and recall

- Responses continue in their destination when you switch chats. There can be
  one active request per chat, with independent requests in other chats.
- Press Escape twice within 1000 ms in chat input to cancel its response.
  Dialogs and terminal focus keep their own Escape behavior. `/cancel` also
  keeps partial text: on disk for persistent chats, in memory for temporary chats.
- Deleting a running persistent destination creates a recovered chat with its
  known context. Later output goes there, not into another selected chat.
- `/fork` selects `Fork of <title>`, copies messages including a partial answer,
  and keeps the provider/model selection. It does not move a running request or
  copy Chat file links. Send or clear pending draft text and inserted documents first.
- `/temp` opens `Ephemeral fork of <title> conversation` in the Wide right sidebar.
  Click a pane or use Alt+Left for main and Alt+Right for temporary. Drafts and
  requests stay separate. Repeating `/temp` focuses the existing temporary pane.
- [X], `/temp close`, or closing the right sidebar deletes the temporary history
  and cancels its request. It is not recovered or included in bulk exports or
  automatic recall. Normal exit saves persistent responses but discards the temporary chat.
- Select skills with inline `@name` mentions or `/skill <name> <request>`.
  `/skills` lists installed skills. Code and quoted examples do not select skills,
  and attached documents do not authorize memory actions.
- `@memory`, `@remember`, and the `@memorize` alias use the full request and
  conversation context to recall facts or save requested facts and configuration.
  Missing facts or uncertain machine identity require clarification; relevant
  topic and machine tags describe saved records.
- Use `@recall the saved details for my home server` or `/skill recall <query>`
  to retrieve existing memories only. The host forbids save, delete, and reindex
  actions, even when another memory skill is mentioned in the same prompt.
  Enable **Settings → Memory → Enable memory** for these skills; this also enables
  automatic memory recall. Memory encryption is a separate `/security` choice.
  Completed memory actions require host results.
- Explicit `@save` and requested memory saves can keep results after a chat closes.
  Automatic memory capture and automatic media saving are off.
- Ask main to “Read the temp chat” or “Read the ephemeral chat” to copy visible
  branch text into its request and saved history. Hidden thinking is excluded.
  Only new branch messages are copied when main is still the source chat;
  otherwise the full temporary history is copied. The copy remains after close.
  Quoted skill markers do not activate skills. `/fork` from temporary creates a
  persistent continuation in main without closing the temporary pane.
- Before sending, TCUI can add up to three relevant excerpts from other local
  chats, with a combined limit of 3,000 characters. These excerpts go to the
  selected provider as untrusted evidence with source IDs. They are not saved
  into the current conversation. Current, deleted, and duplicate content is
  excluded. Matching uses words and a bounded search, not full-history semantic
  memory.

## Web search

- `@websearch <query>` and `@research <query>` use the source selected in
  **Settings → Web search**, even when automatic search is off.
- Local search queries DuckDuckGo Lite directly without a key, browser, or `ddgr`.
  An optional **Local SearXNG endpoint** uses your own JSON-enabled search server.
  Requests run on the TCUI host, including over SSH; local search is not offline.
- `@exa`, `@tavily`, or `@firecrawl` explicitly searches that MCP service.
  Add its key through **Search API keys**, also searchable with `Ctrl+P`.
  Saving a key enables the profile and uses the shared encrypted-vault or
  plaintext-database preference. Environment keys are never imported.
- In the key manager: `e` enables/disables, `h` selects the official hosted HTTP
  preset, `d` deletes a key, and `v` opens the credential vault.
- Automatic search sends each chat request to the selected service and may use
  account credits. Keep secrets out of queries. Explicit same-source mentions
  share one search; failures never silently switch services.
- The result limit is 1–10; Tavily requires at least five. Queries may contain
  at most 2,000 characters. Search evidence includes source URLs and never
  approves terminal commands.

## Commands and terminals

- Recommended commands use an explicit shell fence such as `bash tcui-run`.
  Each completed command block has a top-right copy icon and bottom-left
  **>_ Run in terminal** control. Copy preserves exact commands without fences
  or display wrapping; Run opens review for only that block.
- Ordinary shell examples, quotations, script contents, and unfinished or
  streaming blocks have no command controls. Discussed or rejected commands
  stay inline. Arriving responses and opening review never execute commands.
- Shell commands and arguments stand out in a contrasting color and bold text;
  recognized headings, status labels, and flow keywords keep the normal code color.
  Highlighting never changes copied commands and is not a safety verdict.
- Prefer a short heading followed by each command, grouped in one paste-ready
  block. Conditionals, wrappers, and status reports are for cases that need them.
  Journal/systemd queries retain supported `--no-pager` options.
- The clicked block starts selected; Space or its checkbox toggles it.
  PageUp/PageDown scroll the exact payload.
- In review, **Copy** or `Ctrl+C` copies selected code. **Run** or `Ctrl+R` approves
  those commands in a new clean terminal using the indicated shell.
  Code is sourced from a private temporary script, so interactive prompts cannot
  consume later commands. Shell state persists; normal close removes the script.
- Tab or **Dest** chooses an existing terminal. **Paste** or `Ctrl+V` transfers
  without Enter; inspect the foreground program and submit manually if appropriate.
  Escape cancels. No command is automatically retried.
- Commands run on the machine hosting TCUI. Over SSH, that is the server.
- Terminal header **Tab**, **Right**, **Bottom**, or `F7` changes the same session's
  view. Drag its divider to resize. `F6` switches terminal/chat focus.
- `Ctrl+C` and Escape reach the focused terminal. `Ctrl+P` remains settings search.
  Shift+PageUp/PageDown scroll terminal history.
- Closing live terminals or quitting with them requires confirmation. Sessions
  survive switching views, not application exit.

## Approved commands and reminders

- Mention `@command` to let the agent choose approved programs and literal arguments.
  It reads the conversation and invoked skills, executes tools, and observes results.
  Enter one whitelist/blacklist rule per line in Settings → Terminal; blacklist wins.
  `curl` allows any arguments; `curl -H` requires the exact argument prefix `-H`.
  Quotes preserve spaces. Rules do not normalize flag aliases or reorder arguments.
  Native Pushover delivery is subject to the full argument policy too.
  No shell is used. Command output becomes untrusted chat/model context. Trust
  approved programs and their contents: this is not a sandbox, and does not
  restrict manually approved terminal commands. Quoted examples and history alone
  cannot authorize a command.
- Commands have a 30-second timeout and 64-KiB output limit per stream.
  Cancel the request to terminate its Unix process group.
- `@ping-user Tell me the backup is ready` asks for immediate Pushover delivery.
  Settings → Pushover opens masked user-key and selected app-token entry.
  New/replacement keys are encrypted by default; plaintext storage is an explicit
  choice. Replacing an encrypted key requires unlock. Other saved keys are unchanged.
  One application token is required; additional named tokens are optional.
  Curl must be whitelisted; keys are supplied through standard input.
- The agent can explicitly read Pushover keys from the selected TCUI-keystore or
  environment/file source. **Raw key reads disclose those keys to your AI provider.**
  Encrypted records require unlock; plaintext Pushover records can be read while
  locked. A locked/missing keystore key never falls back to an environment file.
  Other credentials are not exposed through this tool.
- Ask `@remindme via @ping-user in 20 min to review those companies` or
  `Please @ping-user tomorrow at noon to review the backup`. Skills can appear
  anywhere in the current request; no fixed grammar, directive order, or separator
  is required. The agent uses conversation context and actual scheduling results.
  `@schedule` also supports scheduling; ask naturally to list or cancel reminders.
  Completed-action receipts survive a later model failure.
- Scheduling requires the host's user systemd manager. Toolbox timers re-enter the
  originating container. Keep that container, TCUI binary, and curl available.
  Timers can outlive TCUI, but do not promise delivery after logout or reboot.
- Pending Pushover jobs store plaintext messages and keys in private files, even
  when the app is encrypted. Locking or changing the vault password does not revoke
  them. Cancel jobs to remove them. Registered one-shot snapshots are consumed after
  one attempt, including failure; a post-stop hook also handles early callback
  failures. One-shot timer/service units are collected; recurring snapshots remain
  until cancelled. Cleanup errors require recovery or cancellation after restoring
  the executable/container; unknown cadence never authorizes deleting recurring data.
- Back up `reminders.toml` with `reminder.key`; reminder protection is independent
  of the app password so authorized background delivery works while TCUI is locked.

## Tips

- `shift+enter` inserts a newline when typing a multi-line message.
- Conversation history, settings, and credentials are stored locally.
- Connections explicitly choose password-encrypted `tcui-credentials` storage or the
  plaintext application SQLite database. This choice converts all saved provider
  credentials. Machine passwords and SSH keys always remain encrypted.
  TCUI never loads provider keys from environment files.
- Startup offers unlock before opening encrypted sections. If entry or chats are
  protected, unlock is required to continue. Otherwise, you can continue with
  protected sections locked and unlock them later through `/unlock`.
- Secret fields are masked. Paste full SSH private keys into the vault's SSH-key
  value field; multiline terminal paste is supported. `Ctrl+U` clears a field.
- Provider diagnostics are written to `tcui.log` in the app data directory,
  with secrets redacted.

---

Press `Esc` to close this window.
