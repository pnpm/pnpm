---
"pacquet": patch
---

Switching to the pnpm version pinned in `packageManager` no longer writes a temporary file to the project directory, so file watchers no longer see a change on every command [pnpm/pnpm#17681](https://github.com/pnpm/pnpm/issues/17681).
