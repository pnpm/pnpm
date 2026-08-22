---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed `pnpm install --merge-git-branch-lockfiles` deleting the per-branch lockfiles when the `lockfile` setting is `false`. Such an install never reads them, so it has nothing to merge them into and now leaves them alone.
