---
"@pnpm/lockfile.verification": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/cli.default-reporter": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now succeeds when an optional dependency could not be resolved and was skipped by the install that wrote the lockfile. Previously, frozen installs failed with `ERR_PNPM_OUTDATED_LOCKFILE`. The frozen install skips the unresolved optional dependency again. The notice now states that the dependency could not be resolved and names the requested range [#3960](https://github.com/pnpm/pnpm/issues/3960).
