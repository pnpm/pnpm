---
"pnpm": patch
---

`pnpm install` now resolves a `catalog:` peer dependency declared by a local `file:` dependency. Installing such a dependency failed with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` [pnpm/tasks#310](https://github.com/pnpm/tasks/issues/310).
