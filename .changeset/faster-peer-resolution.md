---
"pacquet": minor
---

Made peer resolution significantly faster in large multi-importer workspaces (a 114-importer workspace's resolution dropped from ~77s to ~36s): importers whose hoist rounds converged no longer re-walk their dependency forest every round, later rounds walk only newly added direct dependencies, ownership handovers with an unchanged peer context no longer invalidate shared walk caches, and the resolver's internal hash maps use a faster hash. Peer dependencies provided by multiple candidate versions may resolve to a different (still range-valid) provider than before, which can shift some peer-variant suffixes in `pnpm-lock.yaml` once.
