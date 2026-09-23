---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
"pacquet": patch
---

`pnpm store status` now prints only the modified package paths when packages in the store have been mutated. It no longer suggests running `pnpm install --force` to refetch the modified packages [#919](https://github.com/pnpm/pnpm/issues/919).
