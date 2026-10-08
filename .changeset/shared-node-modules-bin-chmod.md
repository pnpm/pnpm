---
"pacquet": patch
---

`pnpm install` no longer fails with `ERR_PNPM_CMD_SHIM_CHMOD` when `node_modules/.bin` holds a shim that another user created, if everyone can already execute it and it is not world-writable. This happens when several users share one checkout.
