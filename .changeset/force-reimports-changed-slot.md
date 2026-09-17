---
"pacquet": patch
---

`pnpm install --force` now re-imports every package into the virtual store, including packages an earlier install already materialized. A forced install kept the files the earlier install left in place, so it could not repair a package whose contents had drifted [#15030](https://github.com/pnpm/pnpm/issues/15030).
