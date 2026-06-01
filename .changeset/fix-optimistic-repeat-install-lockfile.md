---
"@pnpm/deps.status": patch
"pnpm": patch
---

Fix `pnpm install` with `optimisticRepeatInstall` incorrectly reporting `Already up to date` when `pnpm-lock.yaml` changed but project manifests did not. This affected workflows such as checking out or restoring only the lockfile [#12100](https://github.com/pnpm/pnpm/issues/12100).
