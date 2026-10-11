---
"pacquet": patch
---

`pnpm pipeline` now runs the root's tasks when `pnpm-workspace.yaml` lists only the root in `packages` or omits `packages`, as `pnpm -r run` does. A change to a root file in such a workspace no longer triggers a full-run warning. A `pnpm pipeline --full` run that selects no projects now reports that no projects are selected [#16849](https://github.com/pnpm/pnpm/issues/16849).
