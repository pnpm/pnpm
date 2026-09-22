---
"pacquet": patch
---

Filtered and recursive run and exec commands in workspaces with `sharedWorkspaceLockfile: false` now verify dependencies in the selected projects rather than expecting a root workspace state [pnpm/pnpm#15272](https://github.com/pnpm/pnpm/issues/15272).
