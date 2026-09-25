---
"pacquet": patch
"pnpm": patch
---

`pnpm install -r` now installs every workspace project when `recursiveInstall` is set to `false` in `pnpm-workspace.yaml` [pnpm/pnpm#7504](https://github.com/pnpm/pnpm/issues/7504).
