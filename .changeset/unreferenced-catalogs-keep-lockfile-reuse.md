---
"pacquet": patch
---

`pnpm install`, `pnpm add` and `pnpm update` now reuse `pnpm-lock.yaml` when the workspace configures a catalog entry that no dependency references, such as an unused entry or one whose package an `overrides` entry replaces. Before, every resolution in such a workspace resolved all dependencies again from the registry [#16923](https://github.com/pnpm/pnpm/issues/16923).
