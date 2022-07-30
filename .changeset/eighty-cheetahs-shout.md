---
"@pnpm/lockfile-file": patch
"pnpm": patch
---

Don't incorrectly identify a lockfile out-of-date when the package has a publishConfig.directory field [#5124](https://github.com/pnpm/pnpm/issues/5124).
