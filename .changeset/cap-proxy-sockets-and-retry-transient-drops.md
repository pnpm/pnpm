---
"pacquet": patch
---

`pnpm install` now caps concurrent connections to a proxy at 50 sockets by default, and immediately retries transient connection resets when downloading package archives [pnpm/pnpm#15280](https://github.com/pnpm/pnpm/issues/15280).
