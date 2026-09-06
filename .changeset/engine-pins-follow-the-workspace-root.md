---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reads the `packageManager`, `devEngines.packageManager` and runtime pins from the workspace root's `package.json` when `lockfileDir` is set. It read them from the lockfile directory, so a project that moved its lockfile lost the pins it declared [#14633](https://github.com/pnpm/pnpm/issues/14633).
