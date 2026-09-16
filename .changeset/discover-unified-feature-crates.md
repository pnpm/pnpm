---
"pacquet": patch
---

`pnpm install` now generates a Cargo lockfile when one dependency turns on an optional crate and another asks for a weak feature of it, written `crate?/feature`. pnpm did not fetch the index entry for a crate that only the two together activate [pnpm/pnpm#14960](https://github.com/pnpm/pnpm/issues/14960).
