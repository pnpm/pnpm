---
"@pnpm/deps.status": patch
"pnpm": patch
---

`pnpm install` now refreshes the injected copies of workspace projects when the projects were rebuilt since the last install. It used to report "Already up to date" and leave the copies stale until `pnpm install --force` [#4407](https://github.com/pnpm/pnpm/issues/4407).
