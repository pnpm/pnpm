---
"pacquet": patch
---

`pnpm install` no longer fails on Android (Termux), where the platform does not support file locks. The store operation lock is skipped with a warning there instead of aborting the command [#16508](https://github.com/pnpm/pnpm/issues/16508).
