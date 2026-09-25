---
"@pnpm/resolving.npm-resolver": patch
"pacquet": patch
"pnpm": patch
---

An offline install that fails because its registry metadata cache predates the 11.27 / 12.4 cache-layout change now says so, and names the older mirror on disk [#15656](https://github.com/pnpm/pnpm/issues/15656). The `ERR_PNPM_NO_OFFLINE_META` error code is reported again too.
