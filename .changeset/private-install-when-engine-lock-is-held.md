---
"pacquet": minor
---

When a project pins a pnpm version or a runtime that another pnpm process is installing at that moment, pnpm now waits a few seconds and then installs its own copy into a private directory under the store and runs it from there. It used to wait up to five minutes for the other install and then use the shared install directory without the lock. The private copy is removed once the command has run, and `pnpm store prune` removes any that a killed process left behind [#15413](https://github.com/pnpm/pnpm/issues/15413).
