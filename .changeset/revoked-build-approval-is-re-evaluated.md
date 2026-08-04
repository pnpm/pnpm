---
"pacquet": patch
---

Removing a package from `allowBuilds` now fails the next `pnpm install` under `strictDepBuilds` instead of reporting the project as already up to date. A build whose output is already cached in the store no longer counts as an approval [#11035](https://github.com/pnpm/pnpm/issues/11035).
