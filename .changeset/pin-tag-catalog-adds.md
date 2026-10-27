---
"pacquet": patch
---

`pnpm add` now saves the resolved version range of an npm alias. For example, `pnpm add v2@npm:vue` and `pnpm add v2@npm:vue@^3.0.0` both save `npm:vue@^3.5.13`.

When `pnpm add` adds a dist-tag such as `vue@legacy` to a catalog, the catalog entry records the exact version the tag points to.
