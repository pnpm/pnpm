---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r fetch` now fetches each selected project's lockfile when `sharedWorkspaceLockfile` is `false`. Use `--filter` to limit which projects are fetched [pnpm/pnpm#18152](https://github.com/pnpm/pnpm/issues/18152).
