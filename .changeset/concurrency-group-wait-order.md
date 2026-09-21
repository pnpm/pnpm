---
"pacquet": minor
---

Waiting tasks of a concurrency group now take available slots in arrival order, with higher `priority` tasks going first. If workspaces use different limits for the same group, a later task can take a free slot that earlier tasks cannot use. `pnpm tasks status` lists running and waiting tasks in each group. It also shows how long each task has been running or waiting.
