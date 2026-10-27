---
name: schedule
description: Schedule a local reminder through the host system timer.
---

Use direct host scheduling. Do not call another model or planner.

Accepted forms:

- `@schedule in 10m | Stretch`
- `@schedule at 2026-07-01 09:00 | Join the meeting`
- `@schedule daily 09:00 | Stand up`
- `@schedule weekly mon 09:00 | Review inbox`
- `@schedule calendar Mon..Fri *-*-* 17:30:00 | Wrap up`
- `@schedule list`
- `@schedule forget <reminder-id>`

Keep the reminder message concise. Treat `@remindme` as the same feature.
