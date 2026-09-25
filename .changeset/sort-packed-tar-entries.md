---
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm pack` now writes tarball entries grouped by file extension and file name, the order npm uses. Packages that ship many same-named files, such as template collections, pack much smaller [#14766](https://github.com/pnpm/pnpm/issues/14766).
