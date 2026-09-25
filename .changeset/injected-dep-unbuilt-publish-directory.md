---
"@pnpm/fetching.directory-fetcher": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)
