---
"@pnpm/bins.resolver": patch
"pnpm": patch
---

Resolve `bin` entries that point into `node_modules/...` from the package's real install location instead of the symlinked package path. This fixes global installs of meta-packages like `sudocode`, where command shims previously failed on Windows by probing non-existent `.../cli.js.EXE` paths [#11107](https://github.com/pnpm/pnpm/issues/11107).
