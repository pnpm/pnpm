---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
"pacquet": patch
---

Previously, `pnpm store status` suggested running `pnpm install --force` to refetch modified packages. It now prints only the modified package paths when packages in the store have been mutated [#919](https://github.com/pnpm/pnpm/issues/919).
