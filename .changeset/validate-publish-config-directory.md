---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm pack` and `pnpm publish` now reject `publishConfig.directory` values that use an absolute path or `..` segments outside the package directory.
