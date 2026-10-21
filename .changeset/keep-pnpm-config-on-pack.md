---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now keep `pnpm.config` in the published `package.json`. They also always include the file `pnpm.config` points to in the tarball [#17210](https://github.com/pnpm/pnpm/issues/17210).
