---
"@pnpm/bins.linker": patch
"pnpm": patch
---

Fixed `EEXIST` error when globally installing `node` while a dangling symlink exists from a previous installation.
