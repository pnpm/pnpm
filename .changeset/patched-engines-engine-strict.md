---
"@pnpm/building.during-install": patch
"@pnpm/deps.path": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`engineStrict` now checks the patched `package.json` when a `patchedDependencies` entry changes `engines`. A patch that relaxes `engines.node` no longer fails the install against the published range [pnpm/pnpm#9603](https://github.com/pnpm/pnpm/issues/9603).
