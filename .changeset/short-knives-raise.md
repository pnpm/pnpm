---
"@pnpm/config": patch
"pnpm": patch
---

Convert settings in local `.npmrc` files to their correct types. For instance, `child-concurrency` should be a number, not a string [#5075](https://github.com/pnpm/pnpm/issues/5075).
