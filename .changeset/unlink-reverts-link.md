---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm unlink` now removes the `link:` dependency that `pnpm link <dir>` added to `package.json`, so the linked package is removed from `node_modules` and the lockfile. A `link:` dependency that points to another directory is kept [#4219](https://github.com/pnpm/pnpm/issues/4219).
