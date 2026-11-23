---
"pnpm": patch
---

`pnpm install` now resolves `catalog:` peer ranges declared by a local `file:` dependency through the workspace catalogs. It failed with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` before [#310](https://github.com/pnpm/tasks/issues/310).
