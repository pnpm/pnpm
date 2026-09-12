---
"pacquet": patch
---

`pnpm dedupe` now preserves compatible auto-installed peers when another workspace project depends on a newer major. Repeated runs previously alternated between compatible and incompatible peer versions [pnpm/pnpm#14697](https://github.com/pnpm/pnpm/issues/14697).
