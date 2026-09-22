---
"@pnpm/lockfile.verification": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/cli.default-reporter": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` when an optional dependency could not be resolved and was skipped by the install that wrote the lockfile. The frozen install skips the dependency again and prints the same notice. That notice now says the dependency could not be resolved and names the requested range [#3960](https://github.com/pnpm/pnpm/issues/3960).
