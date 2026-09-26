---
"@pnpm/bins.resolver": patch
"pnpm": patch
"pacquet": patch
---

Command shims for packages that declare binaries inside `node_modules` now locate the target file across virtual store layouts [pnpm/pnpm#11107](https://github.com/pnpm/pnpm/issues/11107).
