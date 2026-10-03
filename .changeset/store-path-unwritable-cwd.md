---
"@pnpm/store.path": patch
"pnpm": patch
---

`pnpm store path`, `pnpm store status`, and other commands that look up the default store no longer fail with `EACCES` or `EPERM` when the current directory is not writable. The store in the pnpm home directory is used instead [#16554](https://github.com/pnpm/pnpm/issues/16554).
