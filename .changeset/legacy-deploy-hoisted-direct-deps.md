---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

A legacy `pnpm deploy` (`--legacy`, `forceLegacyDeploy`, or a workspace without a lockfile) with `nodeLinker: hoisted` now puts the deployed project's direct dependencies at the top of the deployed `node_modules`. Previously another version of a direct dependency, required by one of its dependencies, could take that place, and the version the project declares was written to the source project's `node_modules` instead [#9671](https://github.com/pnpm/pnpm/issues/9671).
