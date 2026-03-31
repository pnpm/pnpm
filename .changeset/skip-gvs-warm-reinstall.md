---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Skip redundant internal linking during GVS warm reinstall when no packages were added. Also filter direct dependency directories by `hasBin` before reading manifests to avoid unnecessary package.json reads.
