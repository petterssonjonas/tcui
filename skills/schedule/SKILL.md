---
name: schedule
description: Schedule a reminder through the host user systemd manager, optionally with Pushover.
---

Use direct host scheduling. Do not call another model or planner.

Accepted forms:

- `@schedule in 10m | Stretch`
- `@schedule at 2030-07-01 09:00 | Join the meeting`
- `@schedule daily 09:00 | Stand up`
- `@schedule weekly mon 09:00 | Review inbox`
- `@schedule calendar Mon..Fri *-*-* 17:30:00 | Wrap up`
- `@schedule to look into subject X tomorrow at noon`
- `@schedule @ping-user tomorrow at 12:00 | Look into subject X`
- `@schedule list`
- `@schedule forget <reminder-id>`
- `@schedule cancel <reminder-id>`

Keep the reminder message concise. Treat `@remindme` as the same feature.

Start the raw instruction with the directive. Put combined directives before the
message; examples, quoted code, and non-leading mentions do not authorize a timer.
Tomorrow resolves the next local date and creates a one-shot timer. Explicit
Pushover scheduling requires enabled delivery, an approved helper, and available
credentials; a locked vault never falls back to environment keys.

Timers can outlive TCUI. Toolbox delivery re-enters the originating container;
the host user systemd manager, container, TCUI path, and helper must remain available.
Pending Pushover snapshots contain plaintext messages and credentials in private
files even if app encryption is enabled. One-shot snapshots are consumed after
the delivery attempt, including failure; recurring snapshots remain until cancellation.
Locking or changing the vault password does not revoke pending snapshots.
Cancel them before rotating captured keys. Report the real scheduling result,
not assumed delivery.
