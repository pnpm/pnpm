---
"@pnpm/deps.status": patch
"pnpm": patch
---

`pnpm install` refreshes injected copies of workspace packages when source projects are rebuilt. Injected copies previously stayed stale until `pnpm install --force` [pnpm/pnpm#4407](https://github.com/pnpm/pnpm/issues/4407).
