---
"pacquet": patch
---

`pnpm install` now links the runtime that an earlier `pnpm install --no-runtime` skipped. A repeat install no longer reports the project as up to date when only the `runtime` setting changed [pnpm/pnpm#17542](https://github.com/pnpm/pnpm/issues/17542).
