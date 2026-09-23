---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
"pacquet": patch
---

`pnpm store status` now lists the modified packages without suggesting `pnpm install --force` to refetch them [#919](https://github.com/pnpm/pnpm/issues/919).
