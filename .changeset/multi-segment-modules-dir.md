---
"pacquet": patch
---

`pnpm install` now puts a workspace project's dependencies under the whole configured `modulesDir`, so a value like `www/modules` lands in `<project>/www/modules`, and `pnpm bin` prints that same directory. A value with one segment, and the default `node_modules`, are unchanged [#15484](https://github.com/pnpm/pnpm/issues/15484).
