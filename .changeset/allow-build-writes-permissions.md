---
"pacquet": patch
---

`pnpm add --allow-build` now records its decision under `permissions` when `pnpm-workspace.yaml` has a `permissions` setting. Before, a `--allow-build=!<pkg>` denial went to `allowBuilds` and was overridden by an existing `permissions.<pkg>.build: true`.
