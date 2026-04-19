---
"@pnpm/config.reader": patch
"pnpm": patch
---

Do not print the `Cannot use both "packageManager" and "devEngines.packageManager" in package.json. "packageManager" will be ignored` warning when the two fields specify the exact same package manager name and version string. This lets projects keep both fields during the migration from `packageManager` to `devEngines.packageManager` without a noisy warning [#11301](https://github.com/pnpm/pnpm/issues/11301).
