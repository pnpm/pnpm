---
"pacquet": patch
---

`pnpm install` no longer fails with `ERR_PNPM_CMD_SHIM_RESOLVE_PATH` when a workspace dependency with a `bin` field is linked to a `publishConfig.directory` that does not exist yet [#16226](https://github.com/pnpm/pnpm/issues/16226).
