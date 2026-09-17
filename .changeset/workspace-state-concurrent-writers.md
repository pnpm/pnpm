---
"pacquet": patch
---

Two pnpm processes installing one workspace at the same time no longer fail on Windows with "Access is denied" while writing `node_modules/.pnpm-workspace-state-v1.json`. The write now retries the transient lock the other process holds, as pnpm's other file writes do.
