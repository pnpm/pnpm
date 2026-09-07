---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reads the `packageManager`, `devEngines.packageManager` and runtime pins from the workspace root's `package.json` when `lockfileDir` is set. A project that moved its lockfile lost the pins it declared there [#14633](https://github.com/pnpm/pnpm/issues/14633).
