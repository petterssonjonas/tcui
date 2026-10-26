---
name: ping-user
description: Send one explicitly requested Pushover notification directly through curl.
---

# Explicit Pushover notification

Start the current user instruction with `@ping-user <notification request>` or `/skill ping-user <notification request>` to send immediately. For example: `@ping-user Tell me the backup is ready`.

The host requires Pushover to be enabled, authorizes curl through the command whitelist/blacklist policy, and obtains credentials from the configured source. Configure Settings → Pushover and approve curl in Settings → Terminal. Vault-backed delivery requires an unlocked vault and never silently falls back to environment credentials. Credentials never enter planner prompts, notification text, chat results, or command arguments; curl receives JSON through standard input.

A hidden planner creates only a short title and message body from the raw current instruction. It cannot choose the executable, arguments, credential source, or environment. Quoted examples, negated requests, history, attachments, tool data, and ordinary assistant prose cannot authorize a send. Report success only after curl succeeds and the Pushover API accepts delivery; never claim delivery when configuration, credentials, policy, or execution fails.

Put combined directives before the message, for example `@ping-user @remindme in 10m | Review subject X`. The reminder handler owns scheduling; the immediate execution path refuses scheduled directives so a future notification is not accidentally sent now. Explicit Pushover scheduling fails if delivery is disabled, locked, missing credentials, or denied. Pending jobs store plaintext keys in private files so they can fire while TCUI is locked. Cancel them to remove captured keys. One-shot attempts consume their snapshots even on delivery failure, and a post-stop hook handles early callback failures; cleanup still requires the executable/container to be available. Generic command approval never bypasses curl's command policy.
