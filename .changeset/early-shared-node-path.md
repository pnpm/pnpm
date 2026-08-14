---
"@pnpm/config.reader": patch
"pnpm": patch
---

Scripts run from a workspace package now resolve `NODE_PATH` against the workspace's shared virtual store when `preferSymlinkedExecutables` is enabled.
