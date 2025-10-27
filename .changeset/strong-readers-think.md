---
"@pnpm/config": patch
"@pnpm/constants": minor
"pnpm": patch
---

Fix a bug in which specifying `filter` on `pnpm-workspace.yaml` would cause pnpm to not detect any projects.
