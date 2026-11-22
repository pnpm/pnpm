---
"pnpm": patch
"@pnpm/installing.commands": patch
"@pnpm/lockfile.fs": patch
---

`pnpm import` respects the symlink policy for the destination lockfile and skips Yarn patches outside the workspace.
