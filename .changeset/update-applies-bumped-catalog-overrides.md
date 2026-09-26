---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm update` now applies an override that references a catalog with the catalog's new value when the update bumps that catalog entry. Before, the packages the override targets kept the old version in the lockfile [#12159](https://github.com/pnpm/pnpm/issues/12159).
