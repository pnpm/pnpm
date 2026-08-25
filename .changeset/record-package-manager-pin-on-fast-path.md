---
"pacquet": patch
---

Record the pnpm version a project pins even when the install has nothing else to do. Adding a `devEngines.packageManager` (or `packageManager`) pin to a project whose dependencies are already installed left `packageManagerDependencies` unwritten, so `pnpm install --frozen-lockfile` failed with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` while a plain `pnpm install` reported "Already up to date" without recording it [#14124](https://github.com/pnpm/pnpm/issues/14124).
