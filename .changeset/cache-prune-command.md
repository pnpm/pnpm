---
"pacquet": minor
---

Added `pnpm cache prune`, which deletes registry metadata cache directories that this version of pnpm can no longer read. Upgrading to pnpm 12.4.0 left one behind for every registry, and no command could remove them [#15046](https://github.com/pnpm/pnpm/issues/15046).

`pnpm cache prune --dry-run` lists what it would delete without removing anything.
