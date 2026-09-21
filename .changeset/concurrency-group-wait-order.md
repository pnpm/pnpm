---
"pacquet": minor
---

Waiting tasks of a concurrency group now start in the order they began waiting. A task with a higher `priority` starts before waiters that arrived earlier. `pnpm concurrency` prints who is running and who is waiting in each group, the task each one is running, and how long each has been running or waiting.
