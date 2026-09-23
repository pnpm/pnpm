---
"pacquet": minor
---

When a project pins a pnpm version or a runtime that another pnpm process is installing at that moment, pnpm now waits a few seconds and then installs and runs a private copy of its own. It used to wait up to five minutes and then use the shared install directory without the lock. The private copy is removed once the command has run, except for a runtime started through a global shim, which keeps its copy until `pnpm store prune` runs. `pnpm store prune` also removes any private copy that a killed process left behind [#15413](https://github.com/pnpm/pnpm/issues/15413).
