---
"@pnpm/fs.packlist": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm pack` now respects workspace-root `.npmignore` and `.gitignore` files when packing workspace packages.
