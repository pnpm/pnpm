---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm add` and `pnpm remove` no longer move unrelated transitive dependencies to other versions. Adding a package and then removing it now leaves `pnpm-lock.yaml` unchanged. Before, the dependencies of auto-installed peers and `npm:` aliased subdependencies could move to a newer version that was already in the lockfile [#11859](https://github.com/pnpm/pnpm/issues/11859).
