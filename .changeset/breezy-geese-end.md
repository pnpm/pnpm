---
"@pnpm/lockfile-utils": patch
"pnpm": patch
---

pnpm should not consider a lockfile out-of-date if `auto-install-peers` is set to `true` and the peer dependency is in `devDependencies` or `optionalDependencies` [#5080](https://github.com/pnpm/pnpm/issues/5080).
