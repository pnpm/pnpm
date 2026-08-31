---
"pacquet": patch
---

An `_auth` credential in an `.npmrc` now authenticates even when its base64 is written without the trailing `=` padding (or with extra padding, or with whitespace inside it), instead of failing with a 401. An `_auth` that is not valid base64, or that carries no `:` between the username and the password, now fails with `ERR_PNPM_AUTH_INVALID_BASE64` / `ERR_PNPM_AUTH_MISSING_SEPARATOR` [#14257](https://github.com/pnpm/pnpm/issues/14257).
