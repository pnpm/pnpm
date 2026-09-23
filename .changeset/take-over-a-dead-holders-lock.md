---
"pacquet": patch
---

pnpm no longer hangs for up to 5 minutes after a pnpm process was killed while setting up the pnpm version pinned in `packageManager` [#15360](https://github.com/pnpm/pnpm/issues/15360). The killed process left behind a lock that every later pnpm command in the project waited on. pnpm now detects that the process holding a lock is gone and takes the lock over at once. The same applies to the locks pnpm takes while installing a managed runtime or writing the global bin directory. Two pnpm processes that are both still running keep waiting for each other as before.
