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

Choose credential storage before entering a key or starting OAuth. Encrypted
and plaintext connections can coexist; encrypted entries carry an encrypted
suffix in their labels. Storage is explicit per record: saving cannot replace
an existing record in the opposite storage mode, and lookups use exact
connection identities. Escape cancels the modal and any pending authorization.
Authorization URLs appear only in the login modal, not in conversation history.

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

Search keys use the same per-record encrypted/plaintext choices and default as
provider credentials; both modes can coexist. Environment API keys are never
imported. Encrypted search keys require the shared password to be unlocked;
plaintext keys do not, unless app-entry protection also gates access. In the
manager, `e` toggles the profile, `h` explicitly restores its hosted HTTP preset,
`d` removes the key and disables the profile, and `v` opens the credential vault.

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

### Selective protection and storage

On first run, and later through `/security`, choose independent protection for
app entry, provider/search credentials, chats, Memories, and the internal
Markdown Vault. App-entry protection alone does not encrypt unchecked stores.
`/unlock` uses one shared password to unlock all selected domains. `/lock`
quiesces tracked app activity, persists pending state, and clears the app's
unlocked state. Voluntary transitions ask before discarding running terminals,
unsaved editors, or drafts; cancelling preserves the current session. Forced
revocation fails closed.

Encrypted chats or protected app entry require a startup password gate before
configuration and stores open; cancelling exits. With only other domains
selected, unprotected surfaces remain usable while protected stores are locked,
and cancelling an optional unlock returns to the app. `/security` remains
available for later changes.

Provider/search records have two explicit storage modes:

- **Encrypted vault**: Argon2id protects a random shared master key using your
  password, a random 16-byte salt, 64 MiB memory, three iterations, and one lane.
  The plaintext master key is not saved to disk. Domain-separated AES-256-GCM
  protects selected content; credential records use fresh 12-byte nonces and
  authenticate their kind, identity, and label.
- **Plaintext SQLite**: an explicit opt-in. Anyone who can read the database can
  read these provider/search keys and tokens. They remain usable while
  credentials are locked, subject to any app-entry gate.

The `/vault` provider/search storage preference sets the default for **new**
records, not a bulk conversion. Changing the credentials checkbox in
`/security` intentionally converts existing provider and search records and
sets that default; leaving it unchanged preserves explicit per-record
exceptions. Machine passwords and SSH keys **always remain encrypted**,
independently of this default or checkbox.

If you initially chose plaintext, the first encrypted record requires shared
password enrollment; reopen `/connect` afterward to add the connection. A
failed protection migration does not reset an already initialized password:
correct the reported prerequisite and retry with the same password.

Wrong passwords and modified encrypted records fail closed. Locking clears
cached unlock material, but cannot recall credentials already handed to a
request or data already sent to a provider. There is no password rotation or
password recovery feature. Keep your password and backups safe.

`/vault` supports:

- Initialize, unlock, and lock; choose the default storage for new credentials.
- Add/edit/delete labeled machine passwords and SSH private keys. Paste the
  complete multiline SSH key into the masked field; Shift+Enter inserts a
  newline and Ctrl+U clears it.
- Copy a selected machine secret with `c`. This is explicit because clipboard
  managers and terminal clipboard integrations may retain copied secrets.
- Explicitly import legacy `keys.toml` credentials after unlocking.

Converting plaintext credentials uses SQLite secure deletion, WAL
checkpoint/truncation, and database compaction, not guaranteed secure erasure.
Flags do not encrypt configuration, connection/profile names, paths, filenames,
secret labels, record types, or logs. Content copied into chats, exports, or
provider requests follows the destination's protection. External originals and
user-kept archives or backups may remain readable; TCUI does not silently rewrite
or delete them. Unix permissions restrict app files to the user, but
these controls do not protect against a compromised account, root, swap, or
filesystem snapshots.

The credential controls in `/vault` are distinct from **Markdown Vault**
content protection. Search Markdown files with `/vault-search <query>`;
existing `@vault` workflows are unchanged.

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

The global `--unlock-password-stdin` reads one UTF-8 password line from
redirected input, not an interactive terminal. Alternatively,
`--unlock-password-fd N` reads from a dedicated inherited descriptor with
`N >= 3` on supported Unix platforms. These options unlock existing shared
protection without selecting credential storage, and cannot be combined with
each other. Never pass passwords as argument values or environment variables.
MCP reserves stdin for JSON-RPC: use the inherited-descriptor option only.

The original inherited password descriptor is marked close-on-exec before any
helper can launch; the separately opened reader is not inherited either.
Password transports are rejected for interactive startup, upgrades, and external
Codex logout rather than ignored. Use the TUI's masked prompt for interactive entry.

App-entry protection applies to auth commands even with `--plaintext`; supply
the shared password to pass that gate. The explicit external Codex logout
described below is exempt because it does not access TCUI's stores.

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

The auth-specific `--vault-password-stdin` is pipe-only to avoid terminal echo.
It initializes or unlocks the shared password and selects encrypted credential
storage. It cannot be combined with `--plaintext` or either global unlock flag.
Supply the final response URL on the next stdin line when using the default
manual callback flow. `--headless` selects Codex's device-code flow or
OpenRouter's manual code/redirect flow; Codex prints a device verification URL
and user code. The TUI provides masked interactive input when piped password
handling is not appropriate.

Inspect or remove TCUI-owned credentials:

```text
tcui auth status [codex|openrouter]
tcui auth logout codex
tcui auth logout openrouter
```

Use a global unlock option for existing protection, or
`--vault-password-stdin` when encrypted credential storage is also intended.
Plaintext credentials need no credential unlock, but do not bypass app-entry
protection. Status never prints keys or tokens. OpenRouter logout removes the
local OAuth credential; Codex logout also attempts native token revocation.

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

The legacy local `chat.key` remains for legacy documents and reminders; it
cannot unlock new shared-master envelopes. With protection configured,
explicit `--decrypt <path>` uses the shared password, including for archived
master-encrypted copies after their managed-domain flag has been disabled.
`--key` is rejected once protection is configured; it applies only to legacy
documents before that cutover. Disabling a managed flag does not make old
encrypted archives readable without the password.
