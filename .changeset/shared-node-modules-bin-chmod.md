---
"pacquet": patch
---

`pnpm install` no longer fails with `ERR_PNPM_CMD_SHIM_CHMOD` when `node_modules/.bin` holds an executable shim that another user created. This happens when several users share one checkout.
