---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now keep `pnpm.config` in the published `package.json` and always include the file it points to in the tarball [#17210](https://github.com/pnpm/pnpm/issues/17210).
