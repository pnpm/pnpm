---
"pacquet": patch
---

`pnpm install` now merges Git conflicts in `pnpm-lock.yaml`. Conflicted lockfiles no longer trigger a fresh dependency resolution. https://github.com/pnpm/pnpm/issues/14880
