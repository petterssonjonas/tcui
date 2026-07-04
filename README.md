# TCUI

TCUI is a terminal chat client for local and hosted LLM providers. It aims to feel like a ChatGPT-style workspace inside a terminal: provider/model selection, conversations, settings, markdown rendering, and terminal-native interaction.

TCUI is functional but still a work in progress and not yet ready for a general release. To try it, clone the repo and run `cargo run`. Feedback is welcome.

## Run

```bash
cargo run
```

## Providers

Configured providers live in the local SQLite database under the app data directory. Built-in providers include Ollama, OpenAI/Codex, OpenRouter, OpenCode Go, OpenCode Zen, Groq, Mistral, Anthropic, Google AI/Gemini, Kilo Gateway, and Berget.ai.

API keys are read from provider env vars, `.env`, saved settings, or supported OAuth token files. Provider diagnostics are written to the app data directory as `tcui.log` with secrets redacted.

## Install

Release assets for every published release:

- `tcui_<version>_amd64.deb` — Debian/Ubuntu package for x86_64
- `tcui_<version>_arm64.deb` — Debian/Ubuntu package for aarch64/arm64
- `tcui-<version>-1.x86_64.rpm` — RPM package for x86_64
- `tcui-x86_64-unknown-linux-gnu.tar.gz` — portable x86_64 binary
- `tcui-aarch64-unknown-linux-gnu.tar.gz` — portable aarch64 binary
- `tcui-<version>-source.tar.gz` — source archive
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

Upgrade in place without touching your config:

```bash
tcui --upgrade
```

## License

TCUI is licensed under GPL-3.0-only. Bundled `potion-base-8M` model assets remain under their
upstream MIT license; the release artifacts ship that license alongside the binary packages.
