---
"pacquet": minor
---

Added `autoDedupe` to deduplicate compatible dependency versions during installation. Enable it in `pnpm-workspace.yaml` or use `pnpm install --auto-dedupe` or `pnpm add --auto-dedupe`. Frozen installs leave the lockfile unchanged.
