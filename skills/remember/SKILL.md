---
name: remember
description: Save one durable fact or preference to local memory.
---

Normalize the information after `@remember` into one concise factual sentence.
Do not call a planner or another model. Do not save secrets, credentials,
temporary requests, speculation, or sensitive third-party information.

Answer the user normally, then end the response with exactly one internal
directive:

```text
<tcui:remember>The concise factual memory.</tcui:remember>
```
