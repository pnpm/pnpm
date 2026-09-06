---
"pacquet": patch
---

The `verifyDepsBeforeRun` check no longer reports a changed workspace structure after a successful install when `sharedWorkspaceLockfile` is false. `pnpm run` and `pnpm exec` now check the project they run in, which is the project the install recorded [#14588](https://github.com/pnpm/pnpm/issues/14588).
