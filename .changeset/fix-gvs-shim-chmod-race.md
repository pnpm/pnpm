---
"pacquet": patch
---

Fixed `ERR_PNPM_CMD_SHIM_CHMOD` when several installs run at once against a shared global virtual store. One install could remove a command shim while another was making it executable ([pnpm/pnpm#14353](https://github.com/pnpm/pnpm/issues/14353)).
