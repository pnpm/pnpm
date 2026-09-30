---
"pacquet": patch
---

`pnpm clean` no longer deletes the project when `virtualStoreDir` or `globalVirtualStoreDir` is set to the project directory. It also leaves a directory outside the project alone when the setting reaches it through a symlink.
