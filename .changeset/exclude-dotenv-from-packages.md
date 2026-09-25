---
"@pnpm/fs.packlist": patch
"pnpm": patch
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now exclude `.env` files from package tarballs [#7826](https://github.com/pnpm/pnpm/issues/7826).
