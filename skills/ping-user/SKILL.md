---
name: ping-user
description: Send one explicitly requested Pushover notification through the configured helper.
---

# Explicit Pushover notification

Start the current user instruction with `@ping-user <notification request>` or `/skill ping-user <notification request>` to send immediately. For example: `@ping-user Tell me the backup is ready`.

The host requires Pushover to be enabled, prepares the configured helper through the command whitelist/blacklist policy, and obtains credentials from the configured source. Vault-backed delivery requires an unlocked vault and never silently falls back to environment credentials. Credentials never enter planner prompts, notification text, chat results, or command arguments.

A hidden planner creates only a short title and message body from the raw current instruction. It cannot choose the helper, arguments, credential source, or environment. Quoted examples, negated requests, history, attachments, tool data, and ordinary assistant prose cannot authorize a send. Report success only after the helper succeeds; never claim delivery when configuration, credentials, policy, or execution fails.

Put combined directives before the message, for example `@ping-user @remindme in 10m | Review subject X`. The reminder handler owns scheduling; the immediate execution path refuses scheduled directives so a future notification is not accidentally sent now. Explicit Pushover scheduling fails if delivery is disabled, locked, missing credentials, or denied. Pending jobs store plaintext keys in private files so they can fire while TCUI is locked. Cancel them to remove captured keys. Generic command approval never bypasses the helper's command policy.
