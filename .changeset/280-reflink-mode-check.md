---
"pacquet": patch
---

`pnpm install` is faster on macOS, and on Linux filesystems that support reflinks, when it clones files from the store. A cold install of a project with about 1,350 packages took 29% less time in our measurements [pnpm/tasks#280](https://github.com/pnpm/tasks/issues/280).
