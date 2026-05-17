---
"pnpm": patch
---

Fix `devEngines.packageManager` not writing `packageManagerDependencies` to `pnpm-lock.yaml` when the lockfile lacks an env-doc entry. Previously the lockfile sync skipped resolution unless an existing `packageManagerDependencies.pnpm` entry needed refreshing, so a fresh install without `onFail: "download"` left the resolved pnpm version unrecorded — contradicting the documented behavior that the resolved version is stored in `pnpm-lock.yaml` [#11674](https://github.com/pnpm/pnpm/issues/11674).
