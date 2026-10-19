---
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when a local directory dependency, such as an injected workspace package, changes a dependency to a different tarball URL or git commit [#17129](https://github.com/pnpm/pnpm/issues/17129).
