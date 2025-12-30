---
"@pnpm/plugin-commands-store": patch
"pnpm": patch
---

`pnpm store prune` should not fail if the dlx cache directory has files, not only directories [#10384](https://github.com/pnpm/pnpm/pull/10384)
