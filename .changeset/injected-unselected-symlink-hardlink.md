---
"pacquet": patch
---

`pnpm install` now materializes a symlink to an unpacked file in an injected workspace dependency as a regular file when hardlinking, and git dependency fetches skip internal directory symlinks when importing files into the store.
