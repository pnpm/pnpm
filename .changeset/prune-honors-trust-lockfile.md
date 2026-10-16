---
"pacquet": patch
---

`pnpm prune` now honors the `trustLockfile` setting and the `--trust-lockfile` flag. With the lockfile trusted, `pnpm prune --prod` works offline when every package is in the store [#16999](https://github.com/pnpm/pnpm/issues/16999).
