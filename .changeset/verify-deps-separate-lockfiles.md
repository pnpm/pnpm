---
"pacquet": patch
---

`pnpm run` and `pnpm exec` now check the project they run in when `sharedWorkspaceLockfile` is false, which is the project the install recorded. `verifyDepsBeforeRun` reported a changed workspace structure on every run in such a workspace, even directly after a successful install [#14588](https://github.com/pnpm/pnpm/issues/14588).
