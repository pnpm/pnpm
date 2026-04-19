---
"@pnpm/config.reader": patch
"pnpm": patch
---

Do not print the `Cannot use both "packageManager" and "devEngines.packageManager" in package.json. "packageManager" will be ignored` warning when the two fields specify compatible versions (same package manager name and the `packageManager` version satisfies the `devEngines.packageManager` version range). This lets projects keep both fields during the migration from `packageManager` to `devEngines.packageManager` without a noisy warning [#11301](https://github.com/pnpm/pnpm/issues/11301).
