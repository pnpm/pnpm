---
"@pnpm/workspace.workspace-manifest-writer": patch
"pnpm": patch
---

`pnpm config delete <key>` no longer fails with `ENOENT` when the config file it would edit does not exist. Clearing a setting that was never set is a no-op [#13651](https://github.com/pnpm/pnpm/issues/13651).
