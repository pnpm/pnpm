---
"@pnpm/global.packages": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update -g` and `pnpm remove -g` now work on a global package whose `node_modules` directory was deleted. They failed with `ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR`, which also blocked every other global package in the same command.
