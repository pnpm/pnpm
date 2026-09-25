---
"pacquet": patch
"@pnpm/building.during-install": patch
"@pnpm/deps.graph-builder": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.package-requester": patch
"@pnpm/lockfile.filtering": patch
"@pnpm/store.controller-types": patch
"pnpm": patch
---

`engineStrict` now checks the patched `package.json` when a `patchedDependencies` entry changes `engines`. A patch that relaxes `engines.node` no longer fails the install against the published range [#9603](https://github.com/pnpm/pnpm/issues/9603).
