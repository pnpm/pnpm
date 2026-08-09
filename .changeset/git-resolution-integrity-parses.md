---
"pacquet": patch
---

A lockfile whose git dependency records an `integrity` (`resolution: {type: git, repo, commit, integrity: sha512-…}`) no longer fails to load with `ERR_PNPM_BROKEN_LOCKFILE` [#13042](https://github.com/pnpm/pnpm/issues/13042). The field is kept on write, so installing no longer churns the lockfile.
