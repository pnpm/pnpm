---
"pnpm": minor
---

Added `pnpm clean` command that safely removes `node_modules` directories from all workspace projects. Unlike manual deletion with `rm -rf` or PowerShell's `Remove-Item -Recurse`, this command correctly handles NTFS junctions on Windows without following them into their targets, preventing catastrophic data loss [#10707](https://github.com/pnpm/pnpm/issues/10707). Use `--lockfile` to also remove `pnpm-lock.yaml` files.
