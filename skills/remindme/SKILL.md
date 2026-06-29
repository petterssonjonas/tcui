---
name: remindme
description: Schedule a local reminder through the host system timer.
---

Use direct host scheduling. Do not call another model or planner.

Accepted forms:

- `@remindme in 10m | Stretch`
- `@remindme at 2026-07-01 09:00 | Join the meeting`
- `@remindme daily 09:00 | Stand up`
- `@remindme weekly mon 09:00 | Review inbox`
- `@remindme calendar Mon..Fri *-*-* 17:30:00 | Wrap up`
- `@remindme list`
- `@remindme forget <reminder-id>`

Keep the reminder message concise. Treat `@schedule` as the same feature.
