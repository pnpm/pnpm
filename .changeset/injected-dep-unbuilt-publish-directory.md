---
"@pnpm/fetching.directory-fetcher": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/fetching.fetcher-base": patch
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.package-requester": patch
"@pnpm/lockfile.verification": patch
"@pnpm/store.cafs-types": patch
"@pnpm/store.controller-types": patch
"@pnpm/store.create-cafs-store": patch
"@pnpm/workspace.injected-deps-syncer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)
