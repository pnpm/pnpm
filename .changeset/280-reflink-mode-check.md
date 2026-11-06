---
"pacquet": patch
---

`pnpm install` is faster when it clones files from the store, which it does by default on macOS. On a Mac with endpoint security software, a cold install of a project with about 1,350 packages took 29% less time [pnpm/tasks#280](https://github.com/pnpm/tasks/issues/280).
