---
"pacquet": patch
---

`_auth` credentials in an `.npmrc` are now decoded and re-encoded before they are sent, so a value written without its `=` padding (or with redundant padding or embedded whitespace) authenticates instead of failing with a 401. An `_auth` that is not base64, or whose decoded form has no `:` separating the username from the password, now fails with `ERR_PNPM_AUTH_INVALID_BASE64` / `ERR_PNPM_AUTH_MISSING_SEPARATOR` instead of being sent as an unusable header [#14257](https://github.com/pnpm/pnpm/issues/14257).
