---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm add` and `pnpm install` now keep the peer dependencies that `pnpm-lock.yaml` records for a package they did not update. A registry whose metadata disagrees with the package's `package.json`, for example by omitting `peerDependenciesMeta`, made `pnpm add` and `pnpm dedupe` write different lockfiles, so `pnpm dedupe --check` failed after `pnpm add` [#16615](https://github.com/pnpm/pnpm/issues/16615).
