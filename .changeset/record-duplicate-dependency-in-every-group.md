---
"@pnpm/installing.context": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.pruner": patch
"@pnpm/lockfile.verification": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` now records a package that `package.json` lists in both `dependencies` and `devDependencies` under both importer sections. `pnpm install --dev` links such a package instead of skipping it, and a lockfile written before this change is updated the next time the project is installed [pnpm/pnpm#9572](https://github.com/pnpm/pnpm/issues/9572).
