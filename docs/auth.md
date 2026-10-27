# Authentication

TCUI supports command-line authentication for Codex and OpenRouter without
starting the interactive TUI. Run `tcui auth --help` for the installed binary's
command summary.

## Important disclosure

TCUI is an independent project. It is not affiliated with, sponsored by, or
endorsed by OpenAI. References to Codex and OpenAI identify the services that
TCUI can connect to; they do not imply a compatibility contract, endorsement,
or trademark permission.

Codex CLI-managed authentication is the recommended path. TCUI reads a valid
Codex CLI credential from `.codex/auth.json` in place and otherwise delegates
login to `codex login` or `codex login --device-auth`. It does not copy or
refresh this externally managed credential: credential maintenance and refresh
remain the Codex CLI's responsibility.

TCUI's native Codex login and subscription transport are **experimental and
non-endorsed**. The OAuth and subscription-backend routes they use are not a
documented third-party compatibility surface. OpenAI or Codex backend changes
may break native login, model discovery, or chat without notice. There is no
formal compatibility promise.

A public comment by a Codex and ChatGPT team member at OpenAI [tolerated a
CLIProxyAPI flow that reused Codex authentication with Claude Code on
2026-07-12](https://x.com/thsottiaux/status/2076119366647894371), including the
statement “If this gets blocked, I owe you a reset.” This is a useful public
signal, not official support, endorsement, trademark permission, or a contract
that TCUI's integration will keep working. TCUI's current transport follows a
[pinned public Codex implementation
reference](https://github.com/openai/codex/blob/c888e8e75a9f0e90ce7d5517f8b9540832cbbf76/codex-rs/model-provider-info/src/lib.rs),
not a promised third-party API.

You authenticate with your own Codex or OpenRouter account. Usage, billing,
entitlements, and rate limits belong to that account and remain subject to the
provider's terms and limits. TCUI does not provide, pool, or proxy accounts.

## Codex: recommended CLI-managed login

Install the Codex CLI using one of its published package-manager commands:

```console
npm i -g @openai/codex
# or
brew install codex
```

Authenticate it directly if desired:

```console
codex login
```

Then let TCUI reuse the existing credential:

```console
tcui auth login codex
tcui auth status codex
```

If `.codex/auth.json` already contains a valid login, the first command reuses
it without running another login. Otherwise, it invokes `codex login`. On
success, TCUI prints the Codex CLI's own output followed by:

```text
Codex CLI credentials are ready for TCUI use.
```

For a headless machine, use:

```console
tcui auth login codex --headless
```

This delegates to `codex login --device-auth`. It still requires the Codex CLI
to be installed. TCUI does not install Codex automatically.

If the experimental native path stops working, return to this recommended
Codex CLI-managed path. That fallback does not imply that TCUI's native flow is
officially supported or endorsed.

## Codex without the Codex CLI: experimental native login

The native path does not require the `codex` executable. It is an independent
TCUI implementation, is explicitly experimental and non-endorsed, and may stop
working without notice.

For a machine with a browser:

1. Run `tcui auth login codex --native`.
2. Read the experimental disclosure printed before authorization.
3. Open the printed `Codex native authorization URL` if it does not open
   automatically, complete authorization in the browser, and return to the
   terminal.
4. Confirm the final `TCUI-native Codex authorization completed.` message.
5. Run `tcui auth status codex`. A native credential reports
   `source=tcui-native` and its expiry without printing token values.

For a fresh headless machine with no Codex CLI installed:

1. Run `tcui auth login codex --native --headless`.
2. Read the experimental disclosure printed before authorization.
3. On another device, open the printed `Codex native verification URL` and
   enter the printed `Codex native device code`.
4. Wait for `TCUI-native Codex authorization completed.`.
5. Run `tcui auth status codex` and verify `source=tcui-native`.

The native flow stores and refreshes only TCUI-owned credentials. If the Codex
CLI is unavailable, this is an optional fallback rather than an officially
supported replacement. If it fails, install the Codex CLI and use the
recommended flow above; neither fallback direction creates an endorsement or
compatibility promise.

## OpenRouter: documented PKCE login

OpenRouter is the supported non-Codex login adapter. TCUI uses OpenRouter's
[documented OAuth PKCE
flow](https://openrouter.ai/docs/guides/overview/auth/oauth) and stores the
exchanged API key as a TCUI-owned credential.

```console
tcui auth login openrouter
tcui auth status openrouter
```

The browser flow prints an `OpenRouter authorization URL`. On success it prints
`OpenRouter authorization completed.`. For a headless terminal, run
`tcui auth login openrouter --headless`, open the printed URL elsewhere, and
paste the redirected URL or authorization code when prompted.

OpenRouter logout removes only the TCUI-owned local credential:

```console
tcui auth logout openrouter
```

## Status and source-aware logout

Inspect all supported providers or one provider:

```console
tcui auth status
tcui auth status codex
tcui auth status openrouter
```

Status reports the provider, credential source/origin, and available expiry or
account-presence metadata. It never prints access tokens, refresh tokens, API
keys, or device codes. Example authenticated source fields are
`source=external-cli`, `source=tcui-native`, and `source=tcui-pkce`.

Ordinary Codex logout removes only a TCUI-owned native credential. It does not
modify `.codex/auth.json` or log the Codex CLI out:

```console
tcui auth logout codex
```

To log out the externally managed Codex CLI credential, opt in explicitly:

```console
tcui auth logout codex --external
```

That command invokes `codex logout`; it therefore affects other tools that use
the same Codex CLI login. TCUI never directly edits or deletes the external
credential file.

## Command reference

The current command forms are:

```text
tcui auth login [OPTIONS] <PROVIDER>
  --headless
  --native

tcui auth logout [OPTIONS] <PROVIDER>
  --external

tcui auth status [PROVIDER]
```

The supported `<PROVIDER>` values are exactly `codex` and `openrouter`.
`--native` selects TCUI's experimental Codex flow; without it, Codex login is
CLI-managed. `--headless` selects a device or pasted-redirect flow appropriate
to the selected provider. `--external` explicitly delegates Codex logout to the
Codex CLI.

The auth command exit codes are: `0` success, `10` unauthenticated, `11` denied
or expired, `12` unsupported, `13` external CLI unavailable, and `14` transport
failure.

## Providers without OAuth login

TCUI does not expose hidden or unofficial OAuth login paths for other
providers:

| Provider | `tcui auth login` support | What to use |
| --- | --- | --- |
| Codex | Recommended CLI-managed login; experimental native login only with `--native` | Commands above |
| OpenRouter | Supported documented PKCE flow | `tcui auth login openrouter` |
| Anthropic / Claude | No OAuth login | Provider API key |
| Google AI / Gemini | No OAuth login; existing passive token-file reads remain unchanged | Existing passive token or provider API key |
| Mistral | No OAuth login | Provider API key |
| Groq | No OAuth login | Provider API key |
| Kilo Gateway | No OAuth login | Provider API key |
| Berget.ai | No OAuth login | Provider API key |
| Other providers | No OAuth login unless listed as supported above | Provider API key |

Configure API keys through the provider's environment variable, a local `.env`
file, or TCUI's saved API-key settings. Do not try unsupported provider names
with `tcui auth login` or reuse unrelated OAuth tokens.

## Storage, migration, and limitations

TCUI-owned Codex OAuth records and OpenRouter PKCE credentials live in TCUI's
existing encrypted `keys.toml` file. On Unix, TCUI writes the file with `0600`
permissions. Encryption and restrictive file permissions reduce exposure, but
the file is not an OS-backed credential vault.

OS keyring integration is future work and is not included in the current auth
implementation. Until then, protect the user account and filesystem that own
the TCUI data directory and its encryption material.

No manual migration is required:

- Existing `keys.toml` files and legacy API-key entries continue to load
  unchanged. The versioned typed credential store is backward-compatible.
- Existing `.codex/auth.json` credentials continue to work unchanged and are
  read in place. TCUI does not copy them into `keys.toml`.
- Existing API-key environment, `.env`, and saved-key behavior remains
  available for providers without OAuth login.
