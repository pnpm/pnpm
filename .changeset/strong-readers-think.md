---
"@pnpm/config": patch
"pnpm": patch
---

Fix a bug in which specifying `filter` on `pnpm-workspace.yaml` would cause pnpm to not detect any projects.
