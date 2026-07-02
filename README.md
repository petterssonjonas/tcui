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

Release assets include:

- `tcui-x86_64-unknown-linux-gnu.tar.gz`
- `.deb`
- `arm64 .deb`
- `.rpm`

Linux install script:

```bash
curl -fsSL https://github.com/petterssonjonas/tcui/releases/latest/download/install.sh | bash
```

The installer verifies the release `SHA256SUMS` file before installing the binary.

When a release is published, the GitHub release workflow builds the release packages and uploads them alongside the install script and checksums.

Upgrade in place without touching your config:

```bash
tcui --upgrade
```

## License

TCUI is licensed under GPL-3.0-only. Bundled `potion-base-8M` model assets remain under their
upstream MIT license; the release artifacts ship that license alongside the binary packages.
