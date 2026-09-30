---
"pacquet": patch
---

`pnpm install` and `pnpm dedupe` now move an optional peer to the version already in the dependency graph when no other package provides its locked version anymore. This applies whether the provider moved the peer up or down. After a bump such as `vue` 3.5.40 to 3.5.43, the lockfile kept a second copy of `@vue/server-renderer` for `@vue/test-utils` [#16443](https://github.com/pnpm/pnpm/issues/16443).
