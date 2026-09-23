---
"@pnpm/releasing.exportable-manifest": patch
"pnpm": patch
"pacquet": patch
---

`pnpm publish` and `pnpm pack` now report a missing `version` or `name` field on a workspace dependency instead of claiming the dependency is not installed [#4164](https://github.com/pnpm/pnpm/issues/4164).
