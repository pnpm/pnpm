---
"pacquet": patch
---

With `gitBranchLockfile` enabled, a branch whose name contains an emoji or another character outside the Basic Multilingual Plane now gets the same lockfile name that pnpm 11 gives it. Previously each such character became one `!` in the file name, while pnpm 11 writes `!!`.
