---
"@pnpm/plugin-commands-setup": patch
"pnpm": patch
---

`pnpm setup` should not fail on Windows if `PNPM_HOME` is not yet in the system registry [#4757](https://github.com/pnpm/pnpm/issues/4757)
