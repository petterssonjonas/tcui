# Providers and credentials

## Add a connection

Open `/providers` or `/connect`, then choose **Add connection**. Only connections
with explicitly saved credentials appear in provider selectors. The preset
catalog is not an automatically connected provider list.

- API-key presets: OpenAI, Anthropic, Google AI, OpenRouter API key, OpenCode Go,
  OpenCode Zen, Mistral, Groq, Kilo Gateway, and Berget.ai. Enter your key; TCUI
  supplies the endpoint.
- **Custom endpoint + API key**: enter a unique connection name, an
  OpenAI-compatible base URL, and a key. HTTPS is required except for loopback
  HTTP. URLs cannot contain credentials, query strings, or fragments.
- **OpenAI OAuth**: native Codex authorization, stored as `Codex`.
- **OpenRouter OAuth**: documented PKCE browser authorization, stored separately
  from the OpenRouter API-key connection.
- **Anthropic Agent SDK**: an Anthropic Console API key, using the official
  Claude CLI subprocess interface described below.

Choose credential storage before entering a key or starting OAuth. Escape
cancels the modal and any pending authorization. Authorization URLs appear
only in the login modal, not in conversation history.

Select a connection and press `m` to enter a model ID, or use `/model <id>`.
This supports custom endpoints without a model-list API. Press `d` in the
provider manager to remove a selected connection after confirmation.

TCUI does **not** discover provider credentials from environment variables,
`.env`, external Codex/Claude/Gemini login files, or an OS keychain. Local
inference configuration remains in Settings; its saved token also uses TCUI's
credential store rather than an environment-variable lookup.

### OAuth locally and over SSH

TCUI does not launch a browser on the application host. The same workflow works
locally or over SSH, without forwarding the callback port:

1. Choose **OpenAI OAuth** or **OpenRouter OAuth** in `/connect`.
2. Press `c` to copy the complete authorization URL. Open it in a browser on
   your own device and complete the provider's sign-in.
3. Copy the complete final response URL from the browser's address bar. A
   `localhost` connection error is expected when your browser is on another
   machine; copy that URL anyway.
4. Paste it into TCUI, or press `p` to open the response field first, then press
   Enter. TCUI validates the callback and exchanges the code using the original
   PKCE verifier. OpenAI callbacks must also match the login's state.

The response field is masked and never enters chat history. Treat authorization
and response URLs as temporary secrets; clipboard managers may retain them.
Automatic loopback completion still works when the browser can reach TCUI.
Escape cancels either completion path.

### Model discovery

OpenAI OAuth loads the authenticated subscription model catalog, including
models unavailable through API-key access. TCUI no longer substitutes a fixed
three-model list or reads another Codex installation's model cache/version.
Discovery runs at startup, after connecting or unlocking, and on `Ctrl+R`.

A successful refresh replaces obsolete selections with the first enabled
model in the provider's priority order. An explicit `/model <id>` selection is
preserved for the current session without adding it to the advertised catalog.
Failed refreshes preserve the last saved catalog; `Ctrl+R` reports discovery
failures so you can check connectivity or unlock `/vault`.

## Web search credentials

Open **Settings → Web search → Search API keys**, or search for **Search API keys**
with `Ctrl+P`. Select Exa, Tavily, or Firecrawl, choose storage, and enter its API
key. Saving enables that MCP profile without adding a chat/model connection.
The automatic search source is selected separately in Web search settings.

Search keys use the same global encrypted/plaintext preference as provider
credentials. Environment API keys are never imported. Encrypted search keys
require an unlocked vault; plaintext keys do not. In the manager, `e` toggles
the profile, `h` explicitly restores its hosted HTTP preset, `d` removes the key
and disables the profile, and `v` opens the credential vault.

`Exa Search`, `Tavily Search`, and `Firecrawl Search` are reserved credential
identities, not chat-provider names. An existing chat connection with a
conflicting name blocks search credential access instead of reusing its key.
HTTP integrations send keys only to the corresponding official HTTPS MCP
origin and never follow redirects. Explicitly configured stdio servers receive
only their selected stored service key, not ambient API credentials; run only
servers you trust.

## tcui-credentials vault

Provider and web search keys, OAuth access/refresh tokens, machine passwords, and SSH private
keys use tables in the application database, normally
`$XDG_DATA_HOME/tcui/tcui.db` (or `~/.local/share/tcui/tcui.db` on Linux).

The storage choice is **global for provider and web search credentials**:

- **Encrypted vault**: Argon2id derives an in-memory key from your password,
  using a random 16-byte salt, 64 MiB memory, three iterations, and one lane.
  AES-256-GCM encrypts records with fresh 12-byte nonces and authenticates their
  kind, identity, and label. The unlock key is not saved to disk.
- **Plaintext SQLite**: an explicit opt-in. Anyone who can read the database can
  read these provider/search keys and tokens. Plaintext credentials remain
  usable while the password vault is locked.

Machine passwords and SSH keys **always remain encrypted**, independently of
provider/search storage preference. Connection names, secret labels, and record types
are database metadata, not encrypted secrets.

An initialized vault prompts for its password on launch. Cancel to continue
without encrypted credentials; `/vault` reopens its controls. Wrong passwords
and modified encrypted records fail closed. Locking removes the cached unlock
key; it does not recall credentials already handed to an in-flight request.
There is no password recovery. Keep your password and backups safe.

`/vault` supports:

- Initialize, unlock, and lock.
- Convert all provider and search records between encrypted and plaintext storage. Unlock
  first when converting existing encrypted records. The plaintext option is
  explicitly labeled **NOT encrypted**.
- Add/edit/delete labeled machine passwords and SSH private keys. Paste the
  complete multiline SSH key into the masked field; Shift+Enter inserts a
  newline and Ctrl+U clears it.
- Copy a selected machine secret with `c`. This is explicit because clipboard
  managers and terminal clipboard integrations may retain copied secrets.
- Explicitly import legacy `keys.toml` credentials after unlocking.

Converting plaintext records to encrypted storage uses SQLite secure deletion,
WAL checkpoint/truncation, and database compaction. This cannot erase filesystem
snapshots, prior backups, or copies outside TCUI. File permissions restrict the
app directory and database to the user on Unix, but encryption does not protect
against a compromised running account.

The credential vault is separate from **Markdown Vault**. Search Markdown files
with `/vault-search <query>`; existing `@vault` workflows are unchanged.

## Anthropic Agent SDK and Console credits

Install/update the official Claude CLI using
[Anthropic's installation instructions](https://code.claude.com/docs/en/setup).
The installed CLI must support `--bare`. Add **Anthropic Agent SDK** and paste an
API key created in your Anthropic Console. The SDK uses API/Console billing and
available API credits, not a Claude subscription login.

For languages other than Python and TypeScript, Anthropic's
[Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview) documents
the CLI subprocess approach. TCUI invokes `claude --bare --print` with streaming
JSON, an isolated working/configuration directory, only the saved Console key,
and no inherited provider credentials. Tools, MCP servers, skills, and session
persistence are disabled: TCUI remains a chat client and owns its conversation
history. Missing or incompatible CLI installations produce an actionable error.

References: [SDK quickstart](https://code.claude.com/docs/en/agent-sdk/quickstart)
and [headless CLI](https://code.claude.com/docs/en/headless).

## OAuth support and disclosure

OpenRouter uses its
[documented PKCE flow](https://openrouter.ai/docs/guides/overview/auth/oauth).
The exchanged API key is a TCUI-owned credential for **OpenRouter OAuth**;
it does not replace the separate **OpenRouter API key** connection.

TCUI is independent and is not affiliated with, sponsored by, or endorsed by
OpenAI. Native OpenAI/Codex OAuth and subscription transport are experimental,
non-endorsed integrations, not a documented third-party compatibility contract.
Provider changes can break login, refresh, discovery, or chat. TCUI never
silently falls back to another application's login. Your account's billing,
entitlements, rate limits, and provider terms apply.

There is no Anthropic subscription OAuth entry. Use an Anthropic Console API
key for direct Anthropic API calls or the Agent SDK entry.

## Command-line authentication

CLI authentication supports `codex` and `openrouter` without opening the TUI.
Successful login registers the corresponding OAuth connection in configuration.
Codex login always uses TCUI's native flow; the old `--native` switch is removed.

By default, both providers print a URL and accept the complete final response
URL on stdin, while also accepting a reachable loopback callback. They do not
launch a browser. Add `--open-browser` only when you explicitly want to launch
a browser on the machine running TCUI; it cannot be combined with `--headless`.

To explicitly choose plaintext provider storage:

```console
tcui auth --plaintext login codex
tcui auth --plaintext login openrouter
```

For encrypted storage, provide the vault password as the first UTF-8 line on
piped stdin, using a trusted password manager or another secure input source:

```text
tcui auth --vault-password-stdin login codex
tcui auth --vault-password-stdin login codex --headless
tcui auth --vault-password-stdin login openrouter --headless
```

The password flag is pipe-only to avoid terminal echo. It initializes or unlocks
the vault and selects encrypted storage. Do not put passwords in shell command
arguments or shell history. Supply the final response URL on the next stdin
line when using the default manual callback flow. `--headless` selects Codex's
device-code flow or OpenRouter's manual code/redirect flow; Codex prints a device
verification URL and user code. The TUI provides masked interactive input when
piped password handling is not appropriate.

Inspect or remove TCUI-owned credentials:

```text
tcui auth status [codex|openrouter]
tcui auth logout codex
tcui auth logout openrouter
```

Pass `--vault-password-stdin` for operations requiring encrypted credentials.
Previously selected plaintext storage does not require an unlock. Status never
prints keys or tokens. OpenRouter logout removes the local OAuth credential;
Codex logout also attempts native token revocation.

`tcui auth logout codex --external` is a separate, explicit request to invoke
`codex logout`. It affects other tools using that external login. Ordinary TCUI
logout does not alter external credentials, and external credentials are never
used for TCUI provider requests.

Exit codes: `0` success, `10` unauthenticated, `11` denied/expired/cancelled,
`12` unsupported, `13` external CLI unavailable, `14` transport or credential
store failure. Use `tcui auth --help` for flags.

## Existing installations

Existing configured provider metadata is preserved, but catalog rows without
saved TCUI credentials do not appear as added connections. Environment keys and
passive external logins no longer authenticate requests. Reconnect explicitly
or unlock `/vault` and choose **Import legacy credentials**.

Legacy import is explicit and transactional. Conflicting records stop the
import without overwriting current credentials. The original legacy file and
its machine key remain untouched; protect those old copies separately. New
credentials are never written to the legacy `keys.toml` store.
