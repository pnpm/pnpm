---
"@pnpm/fs.packlist": patch
"@pnpm/releasing.commands": patch
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm pack` and `pnpm deploy` now preserve internal symlinks in package outputs, while excluding symlinks pointing outside the package [pnpm/pnpm#8208](https://github.com/pnpm/pnpm/issues/8208).
