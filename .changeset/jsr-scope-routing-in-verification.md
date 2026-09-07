---
"pacquet": patch
---

`pnpm install` no longer fails with `ERR_PNPM_META_FETCH_FAIL` on a lockfile that holds a JSR dependency. pnpm reads `@jsr/*` metadata from npm.jsr.io. The supply-chain policy check asked the default registry for it and got a 404 [#14649](https://github.com/pnpm/pnpm/issues/14649).
