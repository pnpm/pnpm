---
"@pnpm/global.packages": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` no longer fail with `ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR` when another global package's directory under `node_modules` is missing. This happened when the package's link into the store dangled after the store was pruned, and it also broke the pnpm install script on such a machine. A package whose directory is missing owns no command shims.
