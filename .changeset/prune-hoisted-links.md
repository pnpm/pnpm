---
"@pnpm/fs.read-modules-dir": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Fixed `pnpm prune` leaving extraneous package links in hoisted installations.
