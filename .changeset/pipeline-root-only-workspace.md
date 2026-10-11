---
"pacquet": patch
---

`pnpm pipeline` now runs the root's tasks when `pnpm-workspace.yaml` lists only the root in `packages` or omits `packages`, as `pnpm -r run` does [#16849](https://github.com/pnpm/pnpm/issues/16849).
