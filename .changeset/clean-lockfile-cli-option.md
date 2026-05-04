---
"pnpm": patch
---

Do not remove `pnpm-lock.yaml` during `pnpm clean` when `lockfile: true` is configured in `pnpm-workspace.yaml`. The lockfile is only removed when the `--lockfile` option is passed to `pnpm clean`.
