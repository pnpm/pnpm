---
"pacquet": patch
---

Installing a workspace now produces the same `pnpm-lock.yaml` every time. Two installs of the same workspace could previously bind a peer dependency to a different — still valid — version, which changed the lockfile without anything in the project changing.
