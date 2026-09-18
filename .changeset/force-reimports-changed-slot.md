---
"pacquet": patch
---

`pnpm install --force` now re-imports every package into the virtual store, so it repairs a package whose files have drifted. A forced install kept the files an earlier install left in place [#15030](https://github.com/pnpm/pnpm/issues/15030).
