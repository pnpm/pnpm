---
"pacquet": patch
---

Ignore the `+<algorithm>.<hash>` build corepack appends when it is written into a `devEngines.packageManager` version. The hash identifies corepack's download rather than a pnpm release, so the version resolved for it never carried one: the lockfile entry could never match its own pin, and `pnpm install --frozen-lockfile` failed with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` on a lockfile that a plain install rewrote identically on every run [#14124](https://github.com/pnpm/pnpm/issues/14124).
