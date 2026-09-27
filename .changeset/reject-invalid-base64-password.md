---
"pacquet": patch
---

pnpm now fails with `ERR_PNPM_AUTH_INVALID_BASE64` when a registry's `_password` in `.npmrc` is not valid base64. Previously it sent the value as the raw password [#16273](https://github.com/pnpm/pnpm/issues/16273).
