---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

`pnpm fetch` no longer fails with `ENOENT` when `nodeLinker` is `hoisted` and the project configures `patchedDependencies` or `allowBuilds`. `pnpm fetch` no longer purges an existing `node_modules` directory [#18037](https://github.com/pnpm/pnpm/issues/18037).
