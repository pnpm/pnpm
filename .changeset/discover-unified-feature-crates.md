---
"pacquet": patch
---

`pnpm install` now generates a Cargo lockfile when one dependency turns on an optional crate and another asks for a weak feature of it, written `crate?/feature`. Resolution failed on such a workspace [pnpm/pnpm#14960](https://github.com/pnpm/pnpm/issues/14960).
