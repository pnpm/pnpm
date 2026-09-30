---
"pacquet": patch
---

`pnpm install` and `pnpm dedupe` now move an optional peer to the version already in the dependency graph when the version it was locked to is no longer provided by any other package. After a bump such as `vue` 3.5.40 to 3.5.43, the lockfile kept a second copy of `@vue/server-renderer` for `@vue/test-utils` [#16443](https://github.com/pnpm/pnpm/issues/16443).
