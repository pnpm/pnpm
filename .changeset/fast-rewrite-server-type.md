---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install` now applies the `serverType` registry setting when it updates the lockfile for an override or catalog change without a full resolution. pnpm wrote the tarball URLs of a declared registry into the lockfile in that case.
