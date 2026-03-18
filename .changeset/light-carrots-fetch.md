---
"@pnpm/exec.prepare-package": major
"@pnpm/fetching.git-fetcher": patch
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/fetching.fetcher-base": minor
"@pnpm/installing.package-requester": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/store.controller-types": minor
"pnpm": patch
---

Block git-hosted dependencies from running prepare scripts unless explicitly allowed in onlyBuiltDependencies [#10288](https://github.com/pnpm/pnpm/pull/10288).
