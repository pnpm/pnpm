---
"pacquet": patch
"pnpm": patch
"@pnpm/releasing.commands": patch
"@pnpm/fs.packlist": patch
---

`pnpm pack` and `pnpm publish` now include bundled workspace dependencies when using the isolated linker. Bundled dependencies are also included when `publishConfig.directory` selects a build directory [pnpm/pnpm#1643](https://github.com/pnpm/pnpm/issues/1643).
