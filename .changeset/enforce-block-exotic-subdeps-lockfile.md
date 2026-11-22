---
"@pnpm/deps.path": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/releasing.commands": patch
"@pnpm/workspace.state": patch
"pnpm": patch
---

`pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.
