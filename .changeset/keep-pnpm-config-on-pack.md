---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now keep `pnpm.config` in the published `package.json`, while still stripping the rest of the `pnpm` field [#17210](https://github.com/pnpm/pnpm/issues/17210).
