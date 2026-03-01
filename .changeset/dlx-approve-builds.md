---
"@pnpm/plugin-commands-script-runners": minor
"@pnpm/global.commands": patch
"pnpm": minor
---

Added interactive approve-builds support to `pnpm dlx`. When build scripts are skipped during installation, pnpm now prompts the user to approve them interactively (same flow as `pnpm add -g`). The `--allow-build` list is no longer part of the dlx cache key, preventing cache fragmentation. On cache hit, if build permissions changed, pnpm performs incremental rebuilds or cache invalidation as needed.
