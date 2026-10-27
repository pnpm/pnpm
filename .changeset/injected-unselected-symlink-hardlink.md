---
"pacquet": patch
---

`pnpm install` now materializes a symlink to an unpacked file in an injected workspace dependency as a regular file when hardlinking. Git dependency fetches now skip internal directory symlinks when importing files into the store.
