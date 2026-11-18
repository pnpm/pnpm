---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy` now installs a workspace package from its publish directory only when the package's `package.json` sets `publishConfig.directory` to that directory. A publish directory that only the lockfile records is ignored.
