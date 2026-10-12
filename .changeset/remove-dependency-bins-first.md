---
"@pnpm/modules-cleaner": patch
"pnpm": patch
---

When pnpm removes or replaces a direct dependency, it now finishes removing the dependency's bins before it deletes the dependency's directory. Packages that declare bins through `directories.bin` no longer leave broken shims in `node_modules/.bin` [#16876](https://github.com/pnpm/pnpm/issues/16876).
