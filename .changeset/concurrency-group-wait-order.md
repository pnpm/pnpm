---
"pacquet": minor
---

Waiting tasks of a concurrency group now start in the order they began waiting. A task with a higher `priority` starts before waiters that arrived earlier. `pnpm concurrency` lists running and waiting tasks in each group. It also shows how long each task has been running or waiting.
