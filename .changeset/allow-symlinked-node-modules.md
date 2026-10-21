---
"@pnpm/fs.symlink-dependency": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now succeeds when `node_modules` is a symlink to an existing directory. Dependency hoisting previously rejected a symlinked `node_modules` as an invalid destination [pnpm/pnpm#17157](https://github.com/pnpm/pnpm/issues/17157).
