---
"pacquet": patch
---

`pnpm self-update` and switching to the `packageManager` version now work behind a proxy that ends a response by closing the TLS connection without a `close_notify` alert. The registry signature check failed there with `ERR_PNPM_PNPM_ENGINE_IDENTITY_UNVERIFIABLE` [#17763](https://github.com/pnpm/pnpm/issues/17763).
