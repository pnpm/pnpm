---
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` now accept a `workspace:` dependency on a project whose version is not valid semver, such as `workspace:1` on a project at version `1`. Before, they reported the lockfile that `pnpm install` had just written as out of date.
