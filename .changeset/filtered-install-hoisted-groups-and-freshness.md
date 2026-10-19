---
"pacquet": patch
---

`pnpm install --filter` with `nodeLinker: hoisted` and `--prod` or `--dev` now preserves previously installed dependency groups for unselected workspace projects [pnpm/tasks#164](https://github.com/pnpm/tasks/issues/164). Repeat filtered installs also re-resolve when a transitive injected workspace dependency or a linked dependency's version changes.
