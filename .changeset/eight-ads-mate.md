---
"@pnpm/plugin-commands-installation": patch
---

`pnpm update pkg` should not fail if `pkg` not found as a direct dependency, unless `--depth=0` is passed as a CLI option [#4122](https://github.com/pnpm/pnpm/issues/4122).
