---
"@pnpm/lockfile.make-dedicated-lockfile": patch
---

`make-dedicated-lockfile` now keeps a dependency on another workspace project linked in the dedicated lockfile. It used to try to fetch that project from the registry, which failed with `ERR_PNPM_FETCH_404` for an unpublished package [#3442](https://github.com/pnpm/pnpm/issues/3442).
