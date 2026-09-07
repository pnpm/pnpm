---
"pacquet": patch
---

Installing a lockfile that holds a JSR dependency no longer fails with `ERR_PNPM_META_FETCH_FAIL`. Every lookup that routes a package by its scope now uses the built-in `@jsr` route to npm.jsr.io. The supply-chain policy check asked the default registry for the `@jsr/*` metadata and got a 404 [#14649](https://github.com/pnpm/pnpm/issues/14649).
