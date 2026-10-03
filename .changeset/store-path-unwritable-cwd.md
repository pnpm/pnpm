---
"@pnpm/store.path": patch
"pnpm": patch
---

`pnpm store path`, `pnpm store status`, and other commands that look up the default store no longer fail when the current directory is not writable. pnpm now uses the store in the pnpm home directory in that case [#16554](https://github.com/pnpm/pnpm/issues/16554).
