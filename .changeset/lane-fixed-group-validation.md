---
"@pnpm/releasing.commands": patch
"@pnpm/releasing.versioning": patch
"pacquet": patch
"pnpm": patch
---

`pnpm lane <lane> --filter <selector>` now rejects partial moves of a `versioning.fixed` group with `ERR_PNPM_VERSIONING_CONFLICTING_CONFIG` before writing `pnpm-workspace.yaml`.
