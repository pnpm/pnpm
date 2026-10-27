---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

`pnpm prune --prod` no longer fails with `ERR_PNPM_EXDEV` when `node_modules` was copied from an earlier Docker build stage [#17424](https://github.com/pnpm/pnpm/issues/17424).
