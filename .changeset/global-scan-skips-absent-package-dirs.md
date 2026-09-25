---
"@pnpm/global.packages": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` no longer fail with `ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR` when another global package's link into the store dangles, for example after the store was pruned. The pnpm install script failed the same way on such a machine.
