# Help

Welcome to **TCUI**, a terminal helper for commands, troubleshooting, and scripts using local or hosted LLM providers.

## Available commands

Type any slash command in the message box:

- `/help` — show this help
- `/keybinds` — show keyboard shortcuts
- `/settings` — open the dedicated Settings tab
- `/terminal` — open a persistent terminal session
- `/providers`, `/connect` — manage added providers, choose a preset or add a custom endpoint
- `/vault` — unlock, initialize or lock the credential vault; choose credential storage; manage machine passwords and SSH keys
- `/vault-search <query>` — search the Markdown Vault
- `/model <id>` — select a model ID manually, including custom endpoints without a model-list API
- `/theme` — change the theme (or `/theme <name>` to apply one directly)
- `/mcp` — open the MCP servers popup
- `/skills` — open the skills popup
- `/web` — toggle automatic web search before each chat message
- `/quit`, `/exit`, `/q` — quit TCUI

## Usage

- Press `Ctrl+P` to search settings and actions. With no query, choose **Settings tab**.
  Search results navigate to the selected setting; the popup does not edit values.
  Settings changes validate and persist individually without a global Save button.
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

- Completed shell code blocks expose **Run** at the top of the assistant response.
  It opens review only; arriving responses and opening review never execute commands.
- Up/Down chooses a block; Space or its checkbox selects it. Only the first block
  starts selected. PageUp/PageDown scroll the exact payload.
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

## Tips

- `shift+enter` inserts a newline when typing a multi-line message.
- Conversation history, settings, and credentials are stored locally.
- Connections explicitly choose password-encrypted `tcui-credentials` storage or the
  plaintext application SQLite database. This choice converts all saved provider
  credentials. Machine passwords and SSH keys always remain encrypted.
  TCUI never loads provider keys from environment files.
- An encrypted vault prompts for its password at launch. Press `Esc` to continue
  without unlocking; reopen with `/vault`. Encrypted connections stay unavailable while locked.
- Secret fields are masked. Paste full SSH private keys into the vault's SSH-key
  value field; multiline terminal paste is supported. `Ctrl+U` clears a field.
- Provider diagnostics are written to `tcui.log` in the app data directory,
  with secrets redacted.

---

Press `Esc` to close this window.
