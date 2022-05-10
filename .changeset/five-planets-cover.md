---
"@pnpm/plugin-commands-setup": patch
"pnpm": patch
---

`pnpm setup` should not override the PNPM_HOME env variable on Windows, unless `--force` is used.
