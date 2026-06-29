# TermChatUI

TermChatUI is a terminal chat client for local and hosted LLM providers. It aims to feel like a ChatGPT-style workspace inside a terminal: provider/model selection, conversations, settings, markdown rendering, and terminal-native interaction.

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
- `.rpm`
- npm package

Linux install script:

```bash
curl -fsSL https://github.com/petterssonjonas/TermChatUI/releases/latest/download/install.sh | bash
```

The installer verifies the release `SHA256SUMS` file before installing the binary.
