---
"pacquet": patch
---

Cargo and Python discovery now skip directories excluded by `!` patterns in `pnpm-workspace.yaml` `packages`. Excluded native projects are no longer parsed or given generated source configuration [pnpm/pnpm#14844](https://github.com/pnpm/pnpm/issues/14844).
