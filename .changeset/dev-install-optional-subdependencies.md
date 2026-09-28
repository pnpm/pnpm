---
"@pnpm/config.reader": patch
"@pnpm/deps.graph-builder": patch
"@pnpm/deps.inspection.tree-builder": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/lockfile.filtering": patch
"@pnpm/lockfile.walker": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).
