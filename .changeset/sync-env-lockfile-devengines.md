---
"pnpm": patch
---

Update the env lockfile's `packageManagerDependencies` entry when `devEngines.packageManager` declares a pnpm version that the lockfile no longer satisfies. Previously, the stale entry was kept even though the running pnpm matched the declared version, silently breaking the integrity record [#11387](https://github.com/pnpm/pnpm/issues/11387).
