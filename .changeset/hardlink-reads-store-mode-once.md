---
"pacquet": patch
---

`pnpm install` no longer reads the mode of a store file before its first hardlink when the store integrity check has just read it. An offline install makes as many `statx` calls as 12.7.0 did [pnpm/pnpm#17500](https://github.com/pnpm/pnpm/issues/17500).
