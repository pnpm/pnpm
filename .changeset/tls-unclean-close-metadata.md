---
"pacquet": patch
---

`pnpm view`, `pnpm update`, and other commands that read registry metadata now work behind proxies that end a response by closing the connection without a TLS `close_notify` alert [#16704](https://github.com/pnpm/pnpm/issues/16704).
