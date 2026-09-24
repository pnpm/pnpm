---
"pacquet": patch
---

A `modulesDir` with several path segments, such as `www/modules`, now puts each workspace project's dependencies in `<project>/www/modules` on both fresh and frozen installs, and `pnpm bin` prints `<project>/www/modules/.bin` [#15484](https://github.com/pnpm/pnpm/issues/15484).
