---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
"pacquet": patch
---

`pnpm store status` now lists only the modified packages when packages in the store were mutated. It previously also suggested running `pnpm install --force` to refetch them [#919](https://github.com/pnpm/pnpm/issues/919).
