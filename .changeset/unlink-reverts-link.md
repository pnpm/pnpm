---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm unlink` now removes the `link:` dependency that `pnpm link <dir>` added to `package.json`. The linked package leaves `node_modules` and the lockfile, and a `link:` dependency to another directory is kept [#4219](https://github.com/pnpm/pnpm/issues/4219).
