---
"@pnpm/config": patch
---

Fixed `lockfile: false` in `pnpm-workspace.yaml` being ignored, causing `pnpm-lock.yaml` to be created despite the setting.
