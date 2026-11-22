---
"@pnpm/fs.read-modules-dir": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm prune` leaving extraneous package links in hoisted installations.
